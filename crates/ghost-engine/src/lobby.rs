use bytes::Bytes;
use ghost_protocol::w3gs::{incoming::ReqJoin, outgoing};

use crate::state::{GamePhase, GameState};
use crate::players::Player;

/// REJECTJOIN reason codes, from src/gameprotocol.rs:249.
pub const REJECT_FULL: u32 = 0x09;
pub const REJECT_STARTED: u32 = 0x0A;
pub const REJECT_WRONG_PASSWORD: u32 = 0x1B;

impl GameState {
    pub(crate) fn handle_req_join(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(idx) = self.pending.iter().position(|(id, _, _)| *id == conn_id) else {
            tracing::debug!(conn_id, "REQ_JOIN from an already-seated connection, ignoring");
            return;
        };
        let (_, link, external_ip) = self.pending.remove(idx);

        let req = match ReqJoin::decode(payload) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(conn_id, error = %e, "malformed REQ_JOIN");
                return;
            }
        };

        // Reject before allocating anything.
        if !matches!(self.phase, GamePhase::Lobby) {
            let _ = link.try_send(outgoing::reject_join(REJECT_STARTED));
            return;
        }
        if self.players.iter().any(|p| p.name.eq_ignore_ascii_case(&req.name)) {
            let _ = link.try_send(outgoing::reject_join(REJECT_FULL));
            return;
        }
        let (Some(sid), Some(pid)) = (self.slots.first_open(), self.players.next_free_pid()) else {
            let _ = link.try_send(outgoing::reject_join(REJECT_FULL));
            return;
        };

        let colour = self.players.next_free_colour(&self.slots);
        let team = sid / 6;
        self.slots.occupy(sid, pid, team, colour);

        let mut player = Player::new(pid, req.name.clone(), conn_id, link);
        player.external_ip = external_ip;
        player.internal_ip = req.internal_ip;
        player.reconnect_key = rand::random();

        // 1. Tell the joiner who they are and what the lobby looks like.
        match outgoing::slot_info_join(
            pid,
            req.listen_port,
            external_ip,
            self.slots.as_wire(),
            self.random_seed,
            self.cfg.map.num_teams,
            self.cfg.map.num_players,
        ) {
            Ok(b) => {
                if player.link.try_send(b).is_err() {
                    self.slots.release(pid);
                    return;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to build slotinfojoin");
                self.slots.release(pid);
                return;
            }
        }

        // 2. Tell the joiner about everyone already here.
        let existing: Vec<(u8, String, [u8; 4], [u8; 4])> = self
            .players
            .iter()
            .map(|p| (p.pid, p.name.clone(), p.external_ip, p.internal_ip))
            .collect();
        for (other_pid, name, ext, int) in existing {
            if let Ok(b) = outgoing::player_info(other_pid, &name, ext, int) {
                let _ = player.link.try_send(b);
            }
        }

        // 3. Map check, so the client can start downloading or confirm it has the map.
        if let Ok(b) = outgoing::map_check(
            &self.cfg.map.path,
            self.cfg.map.size,
            self.cfg.map.info,
            self.cfg.map.crc,
            self.cfg.map.sha1,
        ) {
            let _ = player.link.try_send(b);
        }

        let _ = player.link.try_send(ghost_protocol::gps::init(1, pid, player.reconnect_key, 0));

        self.players.insert(player);

        // 4. Tell everyone else about the joiner.
        if let Ok(b) = outgoing::player_info(pid, &req.name, external_ip, req.internal_ip) {
            for p in self.players.iter_mut() {
                if p.pid != pid {
                    let _ = p.link.try_send(b.clone());
                }
            }
        }
        self.send_all_slot_info();

        tracing::info!(game = %self.cfg.name, %pid, name = %req.name, "player joined");
    }

    pub(crate) fn handle_leave(&mut self, conn_id: u64, reason_code: u32) {
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            p.left = Some(format!("left the game voluntarily (code {reason_code})"));
        } else {
            self.pending.retain(|(id, _, _)| *id != conn_id);
        }
    }

    pub(crate) fn handle_conn_closed(&mut self, conn_id: u64, reason: String) {
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            if p.gproxy {
                if p.disconnected_since.is_none() {
                    p.disconnected_since = Some(std::time::Instant::now());
                    tracing::info!(game = %self.cfg.name, pid = p.pid, "gproxy player disconnected, awaiting reconnect");
                }
            } else if p.left.is_none() {
                p.left = Some(reason);
            }
        } else {
            self.pending.retain(|(id, _, _)| *id != conn_id);
        }
    }
}
