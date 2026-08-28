use std::time::{Duration, Instant};

/// Schedules game ticks on an absolute grid so per-tick lateness never
/// accumulates. Replaces the legacy "sleep 15 ms and compare timestamps" loop.
#[derive(Debug, Clone)]
pub struct TickScheduler {
    period: Duration,
    next: Instant,
}

impl TickScheduler {
    pub fn new(period: Duration) -> Self {
        Self {
            period,
            next: Instant::now() + period,
        }
    }

    #[must_use]
    pub fn deadline(&self) -> Instant {
        self.next
    }

    #[must_use]
    pub fn period(&self) -> Duration {
        self.period
    }

    /// Applied from the next tick onwards; the pending deadline is untouched.
    pub fn set_period(&mut self, period: Duration) {
        self.period = period;
    }

    /// Moves to the next deadline strictly after `now`. Returns how many whole
    /// periods were skipped, which is non-zero only when the process stalled.
    pub fn advance(&mut self, now: Instant) -> u32 {
        self.next += self.period;
        let mut skipped = 0u32;
        while self.next <= now {
            self.next += self.period;
            skipped += 1;
        }
        skipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadlines_do_not_drift_when_ticks_run_late() {
        let mut t = TickScheduler::new(Duration::from_millis(100));
        let first = t.deadline();

        // The tick body took 30 ms; the next deadline is still on the 200 ms grid,
        // not 230 ms. This is the whole point: error must not accumulate.
        let skipped = t.advance(first + Duration::from_millis(30));
        assert_eq!(skipped, 0);
        assert_eq!(t.deadline(), first + Duration::from_millis(100));
    }

    #[test]
    fn drift_stays_bounded_over_many_late_ticks() {
        let mut t = TickScheduler::new(Duration::from_millis(100));
        let first = t.deadline();
        for _ in 0..1000 {
            let now = t.deadline() + Duration::from_millis(5); // always 5 ms late
            t.advance(now);
        }
        // 1000 ticks x 100 ms = 100 s after first deadline.
        assert_eq!(t.deadline(), first + Duration::from_millis(100 * 1000));
    }

    #[test]
    fn reports_skipped_periods_after_a_long_stall() {
        let mut t = TickScheduler::new(Duration::from_millis(100));
        let first = t.deadline();
        // The process stalled for 350 ms: three whole periods were missed.
        let skipped = t.advance(first + Duration::from_millis(350));
        assert_eq!(skipped, 3);
        assert!(t.deadline() > first + Duration::from_millis(350));
    }

    #[test]
    fn changing_latency_takes_effect_from_the_next_tick() {
        let mut t = TickScheduler::new(Duration::from_millis(100));
        let first = t.deadline();
        t.set_period(Duration::from_millis(50));
        assert_eq!(t.period(), Duration::from_millis(50));
        t.advance(first);
        assert_eq!(t.deadline(), first + Duration::from_millis(50));
    }
}
