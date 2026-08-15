use std::time::{Duration, Instant};

use ghost_protocol::w3gs::outgoing;

use crate::state::GameState;

impl GameState {
    /// Returns true while the lag screen is up, meaning no actions go out.
    pub fn check_lag(&mut self) -> bool {
        let limit = self.cfg.sync_limit;
        let game_sync = self.sync_counter;

        let mut newly_lagging: Vec<(u8, u32)> = Vec::new();
        let mut recovered: Vec<(u8, u32)> = Vec::new();

        for p in self.players.iter_mut() {
            let behind = game_sync.saturating_sub(p.sync_counter);
            if p.lagging {
                // Recover only once comfortably caught up (src/game_base.rs:667).
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
        }

        self.lagging = self.players.iter().any(|p| p.lagging);
        self.lagging
    }

    /// Drops anyone stuck on the lag screen longer than `max_lag`.
    pub fn drop_lagging_players(&mut self, max_lag: Duration) {
        for p in self.players.iter_mut() {
            if p.lagging
                && p.left.is_none()
                && p.started_lagging.is_some_and(|t| t.elapsed() >= max_lag)
            {
                p.left = Some(format!("was dropped after lagging for {}s", max_lag.as_secs()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, seated_game};
    use ghost_protocol::w3gs::ids;

    #[test]
    fn a_player_past_the_sync_limit_raises_the_lag_screen() {
        let (mut st, mut rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 5; // 55 ticks behind
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
        st.players.by_pid_mut(2).unwrap().sync_counter = 30; // 30 < 50
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

        // Legacy rule (src/game_base.rs:667): a lagger recovers once it is
        // within half the sync limit, not merely one tick better.
        st.players.by_pid_mut(2).unwrap().sync_counter = 40; // 20 < 50/2
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
}
