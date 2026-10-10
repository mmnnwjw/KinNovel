use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Sliding-window request limiter, ported from `transport.py`'s `RateLimit`:
/// at most `maximum` calls to `wait()` return inside any `window` of time.
/// `wait()` blocks the calling thread until a slot is free.
pub struct RateLimit {
    maximum: usize,
    window: Duration,
    state: Mutex<VecDeque<Instant>>,
    condition: Condvar,
}

impl RateLimit {
    pub fn new(maximum: u32, window: Duration) -> Self {
        RateLimit {
            maximum: maximum.max(1) as usize,
            window: window.max(Duration::from_millis(100)),
            state: Mutex::new(VecDeque::new()),
            condition: Condvar::new(),
        }
    }

    pub fn wait(&self) {
        let mut times = self.state.lock().unwrap();
        loop {
            let now = Instant::now();
            while let Some(&front) = times.front() {
                if now.duration_since(front) >= self.window {
                    times.pop_front();
                } else {
                    break;
                }
            }
            if times.len() < self.maximum {
                times.push_back(now);
                return;
            }
            let front = *times.front().unwrap();
            let elapsed = now.duration_since(front);
            let delay = self
                .window
                .saturating_sub(elapsed)
                .checked_add(Duration::from_millis(20))
                .unwrap_or(Duration::from_millis(20))
                .max(Duration::from_millis(20));
            let (guard, _timeout) = self.condition.wait_timeout(times, delay).unwrap();
            times = guard;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn allows_up_to_maximum_immediately() {
        let limit = RateLimit::new(3, Duration::from_millis(500));
        let start = Instant::now();
        limit.wait();
        limit.wait();
        limit.wait();
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn blocks_until_window_elapses() {
        let limit = Arc::new(RateLimit::new(2, Duration::from_millis(200)));
        limit.wait();
        limit.wait();
        let start = Instant::now();
        limit.wait();
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(150), "elapsed={elapsed:?}");
    }

    #[test]
    fn concurrent_callers_share_the_window() {
        let limit = Arc::new(RateLimit::new(1, Duration::from_millis(150)));
        limit.wait();
        let limit2 = limit.clone();
        let start = Instant::now();
        let handle = thread::spawn(move || {
            limit2.wait();
            start.elapsed()
        });
        let elapsed = handle.join().unwrap();
        assert!(elapsed >= Duration::from_millis(100), "elapsed={elapsed:?}");
    }
}
