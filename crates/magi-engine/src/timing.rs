//! Cancellable, accurate waiting.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Longest uninterrupted nap, which bounds how long a stop request can go unnoticed.
const MAX_NAP: Duration = Duration::from_millis(10);
/// Waits shorter than this are finished by spinning, for sub-millisecond accuracy.
/// Longer waits don't need it and should not burn CPU.
const PRECISE_BELOW: Duration = Duration::from_millis(20);
/// How much of a precise wait is spent spinning.
const SPIN: Duration = Duration::from_micros(400);

/// A flag that asks a run to stop. Cloning shares the flag.
#[derive(Clone, Debug, Default)]
pub struct StopToken {
    stopped: Arc<AtomicBool>,
}

impl StopToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Waits for `duration`. Returns `false` if a stop was requested first.
    pub fn sleep(&self, duration: Duration) -> bool {
        self.sleep_until_with(Instant::now() + duration, duration < PRECISE_BELOW)
    }

    /// Waits until `deadline`. Returns `false` if a stop was requested first.
    pub fn sleep_until(&self, deadline: Instant) -> bool {
        // Deadlines come from timelines where every event should land on time.
        self.sleep_until_with(deadline, true)
    }

    fn sleep_until_with(&self, deadline: Instant, precise: bool) -> bool {
        let spin = if precise { SPIN } else { Duration::ZERO };
        loop {
            if self.is_stopped() {
                return false;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            if left > spin {
                // std's sleep uses a high-resolution timer where the OS has one,
                // so this is accurate to well under a millisecond.
                std::thread::sleep((left - spin).min(MAX_NAP));
            } else {
                std::hint::spin_loop();
            }
        }
    }
}

/// Converts fractional milliseconds to a `Duration`, treating junk as zero.
pub fn millis(ms: f64) -> Duration {
    if ms.is_finite() && ms > 0.0 { Duration::from_secs_f64(ms / 1000.0) } else { Duration::ZERO }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeps_about_as_long_as_asked() {
        let token = StopToken::new();
        let start = Instant::now();
        assert!(token.sleep(Duration::from_millis(15)));
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(15), "woke early: {elapsed:?}");
        assert!(elapsed < Duration::from_millis(60), "woke late: {elapsed:?}");
    }

    #[test]
    fn a_stop_cuts_a_long_sleep_short() {
        let token = StopToken::new();
        let stopper = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            stopper.stop();
        });
        let start = Instant::now();
        assert!(!token.sleep(Duration::from_secs(30)));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn junk_durations_are_zero() {
        assert_eq!(millis(-5.0), Duration::ZERO);
        assert_eq!(millis(f64::NAN), Duration::ZERO);
        assert_eq!(millis(1.5), Duration::from_micros(1500));
    }
}
