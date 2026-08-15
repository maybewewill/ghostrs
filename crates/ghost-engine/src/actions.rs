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
            tracing::info!(game = %self.cfg.name, pid, name = %p.name, "player finished loading");
        }
        self.broadcast(outgoing::game_loaded_others(pid));

        if self.players.iter().all(|p| p.loaded) {
            tracing::info!(game = %self.cfg.name, "all players loaded, game is live");
            self.begin_playing();
        }
    }
    pub fn begin_loading(&mut self) {
        tracing::info!(
            game = %self.cfg.name,
            players = self.players.human_count(),
            "countdown finished, players are loading"
        );
        self.phase = GamePhase::Loading;
        self.started_loading_at = Some(std::time::Instant::now());
        self.delete_virtual_host();
        self.broadcast(outgoing::countdown_start());
        self.broadcast(outgoing::countdown_end());
    }

    pub fn begin_playing(&mut self) {
        self.phase = GamePhase::Playing;
        self.started_loading_at = None;
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
            GamePhase::Countdown {
                started_at,
                total_duration,
                ref mut last_announced_step,
            } => {
                let elapsed = started_at.elapsed();
                if elapsed >= total_duration {
                    self.begin_loading();
                } else {
                    let steps_elapsed =
                        (elapsed.as_millis() / crate::state::COUNTDOWN_STEP.as_millis()) as u8;
                    let step = crate::state::COUNTDOWN_STEPS.saturating_sub(steps_elapsed);
                    if step < *last_announced_step
                        && step >= 1
                        && step <= crate::state::COUNTDOWN_STEPS
                    {
                        *last_announced_step = step;
                        self.send_chat_all(&format!("{step}. . ."));
                    }
                }
            }
            GamePhase::Loading => {
                if let Some(started) = self.started_loading_at {
                    if started.elapsed() >= std::time::Duration::from_secs(60) {
                        tracing::warn!(game = %self.cfg.name, "loading timed out, dropping unready players");
                        for p in self.players.iter_mut() {
                            if !p.loaded && p.left.is_none() && !p.virtual_host {
                                p.left = Some("loading timed out (60s)".into());
                            }
                        }
                        self.reap_left_players();
                    }
                }
            }
            GamePhase::Playing => {
                if self.check_lag() {
                    self.drop_lagging_players(std::time::Duration::from_secs(60));
                    return; // lag screen is up; no actions go out this tick
                }
                self.send_all_actions(skipped);

                // GHost++ game_base.cpp:1059: start gameover timer if only 1 real player remains in game
                let real_players_count = self.players.iter().filter(|p| !p.virtual_host && p.left.is_none()).count();
                if real_players_count <= 1 && self.game_over_time.is_none() {
                    tracing::info!("gameover timer started (one or zero players left)");
                    self.game_over_time = Some(tokio::time::Instant::now());
                }

                // GHost++ game_base.cpp:1067: finish gameover timer after 60 seconds
                if let Some(over_at) = self.game_over_time {
                    if over_at.elapsed() >= std::time::Duration::from_secs(60) {
                        for p in self.players.iter_mut() {
                            if p.left.is_none() && !p.virtual_host {
                                p.left = Some("was disconnected (gameover timer finished)".into());
                            }
                        }
                    }
                }
            }
            GamePhase::Over => self.finished = true,
        }

        // GHost++ game_base.cpp:1089: end game when no players left
        let real_players_count = self.players.iter().filter(|p| !p.virtual_host && p.left.is_none()).count();
        if real_players_count == 0 && matches!(self.phase, GamePhase::Playing | GamePhase::Loading) {
            tracing::info!(game = %self.cfg.name, "no players left, ending game");
            self.phase = GamePhase::Over;
            self.finished = true;
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

        let mut game_over_winner = None;

        for action in queued {
            if let Some(dota) = self.dota.as_mut() {
                if dota.process_action(&action.data) && self.game_over_time.is_none() && game_over_winner.is_none() {
                    game_over_winner = Some(dota.format_winner());
                }
            }
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

        if let Some(winner) = game_over_winner {
            tracing::info!(winner, "gameover timer started (stats class reported game over)");
            self.send_chat_all(&format!("Game over detected! Winner: {winner}. Game will close in 60s."));
            self.game_over_time = Some(tokio::time::Instant::now());
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
    fn countdown_aborts_when_a_player_leaves() {
        let (mut st, mut rxs) = seated_game(2);
        for rx in &mut rxs {
            let _ = drain_ids(rx);
        }
        st.phase = GamePhase::Countdown {
            started_at: std::time::Instant::now(),
            total_duration: crate::state::COUNTDOWN_TOTAL,
            last_announced_step: 5,
        };

        // Mark player 1 as left
        st.players.by_pid_mut(1).unwrap().left = Some("left voluntarily".into());
        st.reap_left_players();

        // Must revert back to Lobby phase (game_base.cpp:1616-1620)
        assert_eq!(st.phase, GamePhase::Lobby);
        // Lobby must be notified via chat, player leave, and updated slot info
        let sent = drain_ids(&mut rxs[1]); // player 2 (pid 2) is still in the lobby
        assert!(sent.contains(&ids::CHAT_FROM_HOST));
        assert!(sent.contains(&ids::PLAYER_LEAVE_OTHERS));
        assert!(sent.contains(&ids::SLOT_INFO));
    }

    #[test]
    fn countdown_progresses_by_wall_clock_time() {
        let (mut st, mut rxs) = seated_game(2);
        st.phase = GamePhase::Countdown {
            started_at: std::time::Instant::now() - std::time::Duration::from_millis(2600),
            total_duration: crate::state::COUNTDOWN_TOTAL,
            last_announced_step: 1,
        };

        st.on_tick(0);
        assert_eq!(st.phase, GamePhase::Loading);
        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::COUNTDOWN_START));
        assert!(sent.contains(&ids::COUNTDOWN_END));
    }

    #[test]
    fn countdown_announces_steps_at_500ms_intervals() {
        let (mut st, mut rxs) = seated_game(2);
        for rx in &mut rxs {
            let _ = drain_ids(rx);
        }
        st.start_countdown("host");
        assert!(matches!(st.phase, GamePhase::Countdown { .. }));

        // Initial tick at t=0 announces "5. . ."
        st.on_tick(0);
        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::CHAT_FROM_HOST));

        // Advance time by 500ms -> step 4
        if let GamePhase::Countdown { ref mut started_at, .. } = st.phase {
            *started_at = std::time::Instant::now() - std::time::Duration::from_millis(500);
        }
        st.on_tick(0);
        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::CHAT_FROM_HOST));
    }

    #[test]
    fn countdown_reaching_zero_starts_loading() {
        let (mut st, mut rxs) = seated_game(1);
        st.start_countdown("slash");
        if let GamePhase::Countdown { ref mut started_at, .. } = st.phase {
            *started_at = std::time::Instant::now() - crate::state::COUNTDOWN_TOTAL;
        }
        st.on_tick(0);
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

    #[test]
    fn player_disconnect_during_loading_starts_game_for_remaining_loaded_players() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_loading();
        // Player 1 sends loaded
        st.handle_loaded(1);
        assert_eq!(st.phase, GamePhase::Loading);

        // Player 2 disconnects without loading
        st.players.by_pid_mut(2).unwrap().left = Some("disconnected".into());
        st.reap_left_players();

        // With Player 2 gone, 100% of seated players (Player 1) are loaded
        assert_eq!(st.phase, GamePhase::Playing);
    }

    #[test]
    fn loading_timeout_drops_unready_players_and_starts_game() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_loading();
        st.started_loading_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(65));
        st.handle_loaded(1);

        st.on_tick(0);
        // Player 2 should be dropped due to timeout, game starts for Player 1
        assert_eq!(st.phase, GamePhase::Playing);
        assert_eq!(st.players.len(), 1);
        assert_eq!(st.players.iter().next().unwrap().pid, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn game_over_triggers_grace_period_and_disconnects_after_60_seconds() {
        let (mut st, mut rxs) = crate::actor::tests_support::seated_game(2);
        st.begin_playing();
        for rx in &mut rxs {
            let _ = crate::actor::tests_support::drain_ids(rx);
        }

        // Inject DotA winner action into action queue
        let mut act = Vec::new();
        act.extend_from_slice(&[0x6b, b'd', b'r', b'.', b'x', 0x00]);
        act.extend_from_slice(b"Global\0Winner\0");
        act.extend_from_slice(&1u32.to_le_bytes()); // Sentinel victory
        st.actions.push(ghost_protocol::w3gs::ActionBlock { pid: 1, data: bytes::Bytes::from(act) });

        st.on_tick(0);

        assert!(st.game_over_time.is_some(), "game_over_time must be set when winner detected");
        // Verify End Message was broadcast
        let chat = rxs[0].try_recv().expect("must receive end chat");
        assert_eq!(chat[1], ghost_protocol::w3gs::ids::CHAT_FROM_HOST);

        // Advance clock by 59 seconds: players must still be connected
        tokio::time::advance(std::time::Duration::from_secs(59)).await;
        st.on_tick(0);
        assert_eq!(st.players.iter().filter(|p| p.left.is_none()).count(), 2);

        // Advance clock past 60 seconds: remaining players must be stopped
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        st.on_tick(0);
        assert_eq!(st.players.iter().filter(|p| p.left.is_none()).count(), 0);
        assert!(st.finished, "game must transition to finished when all players stopped");
    }
}
