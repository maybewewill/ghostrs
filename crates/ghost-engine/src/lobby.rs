use bytes::Bytes;
use ghost_protocol::w3gs::{incoming::ReqJoin, outgoing};

use crate::state::{GamePhase, GameState};
use crate::players::Player;

/// REJECTJOIN reason codes, from src/gameprotocol.rs:249.
pub const REJECT_FULL: u32 = 0x09;
pub const REJECT_STARTED: u32 = 0x0A;
pub const REJECT_WRONG_PASSWORD: u32 = 0x1B;

/// GHost++'s global slot ceiling, not this game's configured `num_slots`.
/// `gameslot.h:39`: `const int MAX_SLOTS = 24;`. `game_base.cpp:2052` gates
/// virtual-host eviction on this constant, not on the current map's slot count.
pub const MAX_SLOTS: usize = 24;

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

        self.slots.occupy_slot(sid, pid);

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
            self.cfg.map.layout_style,
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

        // game_base.cpp:2052: `if (GetNumPlayers() >= MAX_SLOTS-1 || EnforcePID ==
        // m_VirtualHostPID) DeleteVirtualHost();`. MAX_SLOTS is the engine-wide
        // constant (24), not this game's `num_slots` — for a normal, say,
        // 12-slot DotA lobby this branch never fires at join time; GHost++
        // removes the virtual host unconditionally when loading starts instead
        // (game_base.cpp:3389, mirrored in `begin_loading`). The virtual host
        // never occupies a slot (`create_virtual_host` does not call
        // `occupy_slot`), so it was never competing with real players for a
        // seat, only for a PID.
        //
        // The C++'s second condition, `EnforcePID == m_VirtualHostPID`, only
        // ever fires when replaying a saved game (`EnforcePID` defaults to 255
        // and is set only inside `if (m_SaveGame)`, game_base.cpp:1908-1922);
        // ghostrs has no save-game/replay-enforced-layout feature, so there is
        // no `EnforcePID` to compare here. It is intentionally not ported.
        if self.players.human_count() >= MAX_SLOTS - 1 {
            self.delete_virtual_host();
        }

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

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn the_virtual_host_is_announced_to_a_joining_player() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(0);
        st.create_virtual_host();
        assert_ne!(st.virtual_host_pid, 255, "a virtual host PID must be allocated");

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        st.add_conn(7, ghost_net::PlayerLink::for_test(tx), [127, 0, 0, 1]);
        st.handle_req_join(7, &crate::actor::tests_support::reqjoin_bytes("alice"));

        let ids = crate::actor::tests_support::drain_ids(&mut rx);
        let vh = st.virtual_host_pid;
        assert!(
            ids.contains(&ghost_protocol::w3gs::ids::PLAYER_INFO),
            "joiner must be told about the virtual host, got {ids:?}"
        );
        assert_eq!(st.players.by_pid(vh).map(|p| p.name.as_str()), Some(st.cfg.virtual_host_name.as_str()));
    }

    #[tokio::test]
    async fn bot_chat_is_sent_from_the_virtual_host_pid() {
        let (mut st, mut rxs) = crate::actor::tests_support::seated_game(1);
        st.create_virtual_host();
        // Drain P1's own join packets and the virtual host's PLAYER_INFO
        // announcement so only the chat packet is left to inspect.
        crate::actor::tests_support::drain_ids(&mut rxs[0]);
        st.send_chat_all("hello");
        let pkt = rxs[0].try_recv().expect("chat packet");
        // [0xF7, 0x0F, len_lo, len_hi, recipient_count, to_pids..., from_pid, ...]
        // (gameprotocol.cpp:526-542): a count byte and the recipient list
        // precede from_pid, so with one recipient it lands at offset 6.
        assert_eq!(pkt[1], ghost_protocol::w3gs::ids::CHAT_FROM_HOST);
        assert_eq!(pkt[6], st.virtual_host_pid, "sender must be the virtual host, not 255");
    }

    #[tokio::test]
    async fn a_normal_lobby_filling_up_keeps_the_virtual_host_until_loading_starts() {
        // game_base.cpp:2052 gates eviction on the engine-wide MAX_SLOTS (24),
        // not on this game's `num_slots`. A 12-slot lobby filling every seat
        // but one (11 real players) never reaches MAX_SLOTS-1 = 23, so the
        // virtual host must still be present — only `begin_loading` removes
        // it (game_base.cpp:3389, already covered by a separate test).
        //
        // The superseded `num_slots - 1` expression would have deleted the
        // virtual host as soon as `real_players >= 11`, which this exact
        // scenario reaches: this test fails against that old expression.
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(0);
        st.create_virtual_host();
        let vh = st.virtual_host_pid;
        for i in 0..(st.cfg.num_slots - 1) {
            let (tx, _rx) = tokio::sync::mpsc::channel(64);
            st.add_conn(100 + i as u64, ghost_net::PlayerLink::for_test(tx), [0; 4]);
            st.handle_req_join(100 + i as u64, &crate::actor::tests_support::reqjoin_bytes(&format!("p{i}")));
        }
        assert_eq!(st.virtual_host_pid, vh, "virtual host must survive a normal lobby filling up");
        assert!(st.players.by_pid(vh).is_some());
    }
}
