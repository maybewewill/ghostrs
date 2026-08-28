use std::time::{Duration, Instant};

use spectre_protocol::w3gs::outgoing;

use crate::state::GameState;

impl GameState {
    pub fn check_lag(&mut self) -> bool {
        let limit = self.cfg.sync_limit;
        let game_sync = self.sync_counter;
        let mut newly_lagging: Vec<(u8, u32)> = Vec::new();
        let mut recovered: Vec<(u8, u32)> = Vec::new();
        let was_lagging = self.lagging;

        for p in self
            .players
            .iter_mut()
            .filter(|p| !p.virtual_host && p.left.is_none())
        {
            let behind = game_sync.saturating_sub(p.sync_counter);
            if p.lagging {
                if behind < limit / 2 {
                    p.lagging = false;
                    let lag_ms = p
                        .started_lagging
                        .take()
                        .map(|t| t.elapsed().as_millis() as u32)
                        .unwrap_or(0);
                    recovered.push((p.pid, lag_ms));
                }
            } else if behind > limit {
                p.lagging = true;
                p.started_lagging = Some(Instant::now());
                newly_lagging.push((p.pid, 0));
            }
        }

        for (pid, lag_ms) in recovered {
            tracing::info!(game = %self.cfg.name, pid, "player stopped lagging");
            self.broadcast(outgoing::stop_lag(pid, lag_ms));
        }

        if !newly_lagging.is_empty() {
            tracing::info!(
                game = %self.cfg.name,
                laggers = ?newly_lagging.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
                "lag screen raised"
            );
            match outgoing::start_lag(&newly_lagging) {
                Ok(b) => self.broadcast(b),
                Err(e) => tracing::warn!(error = %e, "failed to build start_lag"),
            }
            if !was_lagging {
                self.last_lag_screen_reset = Instant::now();
            }
        }

        self.lagging = self
            .players
            .iter()
            .filter(|p| !p.virtual_host)
            .any(|p| p.lagging);

        let reset_interval = Duration::from_secs(60);
        if self.lagging && self.last_lag_screen_reset.elapsed() >= reset_interval {
            let laggers: Vec<(u8, u32)> = self
                .players
                .iter()
                .filter(|p| p.lagging && p.left.is_none() && !p.virtual_host)
                .map(|p| {
                    (
                        p.pid,
                        p.started_lagging
                            .map(|t| t.elapsed().as_millis() as u32)
                            .unwrap_or(0),
                    )
                })
                .collect();

            if !laggers.is_empty() {
                for &(pid, lag_ms) in &laggers {
                    self.broadcast(outgoing::stop_lag(pid, lag_ms));
                }
                if let Ok(act) = outgoing::incoming_action(&[], 0) {
                    self.broadcast(act);
                }
                if let Ok(b) = outgoing::start_lag(&laggers) {
                    self.broadcast(b);
                }
                self.last_lag_screen_reset = Instant::now();
            }
        }

        self.lagging
    }

    pub fn drop_lagging_players(&mut self, max_lag: Duration) {
        let to_drop: Vec<(u8, String)> = self
            .players
            .iter()
            .filter(|p| {
                !p.virtual_host
                    && p.lagging
                    && p.left.is_none()
                    && p.started_lagging.is_some_and(|t| t.elapsed() >= max_lag)
            })
            .map(|p| {
                (
                    p.pid,
                    format!("was dropped after lagging for {}s", max_lag.as_secs()),
                )
            })
            .collect();

        for (pid, reason) in to_drop {
            self.kick_player(
                pid,
                &reason,
                spectre_protocol::w3gs::ids::PLAYERLEAVE_DISCONNECT,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, seated_game};
    use spectre_protocol::w3gs::ids;

    #[test]
    fn a_player_past_the_sync_limit_raises_the_lag_screen() {
        let (mut st, mut rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 5;
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }

        assert!(st.check_lag(), "lag screen must be up");
        assert!(st.lagging);
        assert!(st.players.by_pid(2).unwrap().lagging);
        assert!(drain_ids(&mut rxs[0]).contains(&ids::START_LAG));
    }

    #[test]
    fn no_lag_screen_while_everyone_is_within_the_limit() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 30;
        assert!(!st.check_lag());
        assert!(!st.lagging);
    }

    #[test]
    fn catching_up_halfway_clears_the_lag_screen() {
        let (mut st, mut rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 5;
        assert!(st.check_lag());
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }

        st.players.by_pid_mut(2).unwrap().sync_counter = 40;
        assert!(!st.check_lag());
        assert!(!st.lagging);
        assert!(drain_ids(&mut rxs[0]).contains(&ids::STOP_LAG));
    }

    #[test]
    fn a_player_lagging_past_the_timeout_is_dropped() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 5;
        st.check_lag();
        st.players.by_pid_mut(2).unwrap().started_lagging =
            Some(Instant::now() - Duration::from_secs(120));

        st.drop_lagging_players(Duration::from_secs(60));
        st.reap_left_players();

        assert_eq!(st.players.len(), 1);
    }

    #[test]
    fn lag_screen_resets_after_60_seconds_with_nonzero_lag_time() {
        let (mut st, mut rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 5;
        assert!(st.check_lag());

        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }

        let past = Instant::now() - Duration::from_secs(61);
        st.last_lag_screen_reset = past;
        st.players.by_pid_mut(2).unwrap().started_lagging = Some(past);

        assert!(st.check_lag());

        let mut packets = Vec::new();
        while let Ok(pkt) = rxs[0].try_recv() {
            packets.push(pkt);
        }

        let ids: Vec<u8> = packets.iter().map(|p| p[1]).collect();
        assert!(
            ids.contains(&ids::STOP_LAG),
            "must send STOP_LAG on reset, got {ids:?}"
        );
        assert!(
            ids.contains(&ids::START_LAG),
            "must send START_LAG on reset, got {ids:?}"
        );

        let start_lag_pkt = packets.iter().find(|p| p[1] == ids::START_LAG).unwrap();
        let num_laggers = start_lag_pkt[4];
        assert_eq!(num_laggers, 1);
        let lag_time = u32::from_le_bytes([
            start_lag_pkt[6],
            start_lag_pkt[7],
            start_lag_pkt[8],
            start_lag_pkt[9],
        ]);
        assert!(
            lag_time >= 60_000,
            "lag_time must be >= 60000 ms, got {lag_time}"
        );
    }
}
