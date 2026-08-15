use bytes::Bytes;
use ghost_protocol::w3gs::{ActionBlock, incoming::OutgoingAction, outgoing};

use crate::state::{GamePhase, GameState};

/// Actions beyond this many wire bytes spill into an INCOMING_ACTION2 packet.
/// Matches src/game_base.rs:988.
pub const MAX_ACTION_PAYLOAD: usize = 1400;

impl GameState {
    pub fn handle_action(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        match OutgoingAction::decode(payload) {
            // The body is a slice of the read buffer: queuing it costs a
            // refcount bump, and it is relayed without ever being parsed.
            Ok(a) => {
                if let Some(dota) = &mut self.dota {
                    dota.process_action(&a.data);
                }
                if let Some(w3mmd) = &mut self.w3mmd {
                    w3mmd.process_action(&a.data);
                }
                self.actions.push(ActionBlock { pid, data: a.data });
            }
            Err(e) => tracing::debug!(conn_id, error = %e, "malformed action"),
        }
    }

    pub fn handle_keepalive(&mut self, conn_id: u64, payload: &Bytes) {
        if ghost_protocol::w3gs::incoming::decode_keepalive(payload).is_err() {
            return;
        }
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            p.sync_counter = p.sync_counter.saturating_add(1);
        }
    }

    pub fn handle_loaded(&mut self, conn_id: u64) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        if let Some(p) = self.players.by_pid_mut(pid) {
            p.loaded = true;
        }
        self.broadcast(outgoing::game_loaded_others(pid));

        if self.players.iter().all(|p| p.loaded) {
            tracing::info!(game = %self.cfg.name, "all players loaded, game is live");
            self.begin_playing();
        }
    }
    pub fn begin_loading(&mut self) {
        self.phase = GamePhase::Loading;
        self.delete_virtual_host();
        self.broadcast(outgoing::countdown_start());
        self.broadcast(outgoing::countdown_end());
    }


    pub fn begin_playing(&mut self) {
        self.phase = GamePhase::Playing;
        for p in self.players.iter_mut() {
            p.loaded = true;
            p.sync_counter = 0;
        }
        self.sync_counter = 0;
        self.game_ticks = 0;

        if let Some(hcl) = &self.hcl {
            let host_pid = self.players.iter().next().map(|p| p.pid).unwrap_or(1);
            let hcl_actions = crate::hcl::Hcl::encode_hcl_actions(hcl, host_pid);
            self.actions.extend(hcl_actions);
            tracing::info!(hcl = %hcl, "injected HCL game mode actions on match start");
        }
    }

    /// One scheduled tick. `skipped` counts periods lost to a stall.
    pub fn on_tick(&mut self, skipped: u32) {
        self.pump_downloads();
        self.reap_gproxy_timeouts(self.cfg.reconnect_wait);
        match self.phase {
            GamePhase::Lobby => {}
            GamePhase::Countdown { remaining } => {
                if remaining == 0 {
                    self.begin_loading();
                } else {
                    self.phase = GamePhase::Countdown { remaining: remaining - 1 };
                }
            }
            GamePhase::Loading => {}
            GamePhase::Playing => {
                if self.check_lag() {
                    self.drop_lagging_players(std::time::Duration::from_secs(60));
                    return; // lag screen is up; no actions go out this tick
                }
                self.send_all_actions(skipped);
            }
            GamePhase::Over => self.finished = true,
        }

        if self.players.is_empty() && matches!(self.phase, GamePhase::Playing | GamePhase::Loading) {
            tracing::info!(game = %self.cfg.name, "no players left, ending game");
            self.phase = GamePhase::Over;
        }
    }

    /// Encodes the tick's action packets once and shares them with every player.
    pub fn send_all_actions(&mut self, skipped: u32) {
        let latency_ms = self.tick.period().as_millis() as u32;
        // A skipped period still advanced game time; report the real interval so
        // clients keep their simulation aligned with ours.
        let elapsed = latency_ms.saturating_mul(skipped + 1);
        let send_interval = elapsed.min(u16::MAX as u32) as u16;

        self.game_ticks = self.game_ticks.wrapping_add(elapsed);
        self.sync_counter = self.sync_counter.wrapping_add(1);

        let queued = std::mem::take(&mut self.actions);
        let mut batch: Vec<ActionBlock> = Vec::new();
        let mut batch_len = 0usize;

        for action in queued {
            let len = action.wire_len();
            if batch_len + len > MAX_ACTION_PAYLOAD && !batch.is_empty() {
                match outgoing::incoming_action2(&batch) {
                    Ok(b) => self.broadcast(b),
                    Err(e) => tracing::warn!(error = %e, "failed to build overflow packet"),
                }
                batch.clear();
                batch_len = 0;
            }
            batch_len += len;
            batch.push(action);
        }

        // The main packet always goes out, even empty: it is the clock tick.
        match outgoing::incoming_action(&batch, send_interval) {
            Ok(b) => {
                if let Some(r) = &self.relay {
                    r.push(b.clone());
                }
                self.broadcast(b);
            }
            Err(e) => tracing::warn!(error = %e, "failed to build action packet"),
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, seated_game};
    use ghost_protocol::w3gs::ids;

    #[test]
    fn a_playing_tick_broadcasts_one_action_packet_per_player() {
        let (mut st, mut rxs) = seated_game(3);
        st.begin_playing();
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }

        st.on_tick(0);

        for rx in rxs.iter_mut() {
            assert_eq!(drain_ids(rx), vec![ids::INCOMING_ACTION]);
        }
        assert_eq!(st.sync_counter, 1);
        assert_eq!(st.game_ticks, 100);
    }

    #[test]
    fn queued_actions_are_flushed_and_the_queue_is_emptied() {
        let (mut st, mut rxs) = seated_game(2);
        st.begin_playing();
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }
        st.actions.push(ActionBlock { pid: 1, data: Bytes::from_static(&[0x10, 0x20]) });

        st.on_tick(0);

        assert!(st.actions.is_empty(), "actions must not be replayed next tick");
        let first = rxs[0].try_recv().expect("action packet");
        assert_eq!(first[1], ids::INCOMING_ACTION);
        assert!(first.len() > 8, "packet must carry the action body and CRC");
    }

    #[test]
    fn oversized_action_batches_spill_into_incoming_action2() {
        let (mut st, mut rxs) = seated_game(1);
        st.begin_playing();
        let _ = drain_ids(&mut rxs[0]);

        // 20 x 100-byte actions = 2060 wire bytes, past the 1400-byte limit.
        for _ in 0..20 {
            st.actions.push(ActionBlock { pid: 1, data: Bytes::from(vec![7u8; 100]) });
        }
        st.on_tick(0);

        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::INCOMING_ACTION2), "overflow packet must be sent");
        assert_eq!(sent.last(), Some(&ids::INCOMING_ACTION), "main packet goes last");
    }

    #[test]
    fn keepalive_advances_the_players_sync_counter() {
        let (mut st, _rxs) = seated_game(1);
        st.begin_playing();
        st.on_tick(0);
        assert_eq!(st.players.by_pid(1).unwrap().sync_counter, 0);

        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_u8(&mut p, 0);
        bytes::BufMut::put_u32_le(&mut p, 0xDEAD);
        st.handle_keepalive(1, &p.freeze());

        assert_eq!(st.players.by_pid(1).unwrap().sync_counter, 1);
    }

    #[test]
    fn lobby_ticks_do_not_produce_action_packets() {
        let (mut st, mut rxs) = seated_game(2);
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }
        st.on_tick(0);
        for rx in rxs.iter_mut() {
            assert!(!drain_ids(rx).contains(&ids::INCOMING_ACTION));
        }
    }

    #[test]
    fn countdown_reaching_zero_starts_loading() {
        let (mut st, mut rxs) = seated_game(1);
        st.start_countdown("slash");
        for _ in 0..6 {
            st.on_tick(0);
        }
        assert_eq!(st.phase, GamePhase::Loading);
        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::COUNTDOWN_START));
        assert!(sent.contains(&ids::COUNTDOWN_END));
    }

    #[test]
    fn all_players_loaded_moves_the_game_to_playing() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_loading();
        st.handle_loaded(1);
        assert_eq!(st.phase, GamePhase::Loading);
        st.handle_loaded(2);
        assert_eq!(st.phase, GamePhase::Playing);
    }
}
