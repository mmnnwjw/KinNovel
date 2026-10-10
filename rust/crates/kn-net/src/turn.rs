use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Port of `transport.py`'s `_acquire_turn`/`_release_turn`: only one
/// invocation may actually be in flight on the shared socket at a time, and
/// a lower priority number (0 = interactive) always wins over any higher
/// (less urgent, e.g. prefetch = 1) number that is also waiting.
pub struct TurnScheduler {
    state: Mutex<State>,
    condition: Condvar,
}

struct State {
    active: bool,
    waiting: HashMap<i32, u32>,
    shutdown: bool,
}

impl TurnScheduler {
    pub fn new() -> Self {
        TurnScheduler {
            state: Mutex::new(State { active: false, waiting: HashMap::new(), shutdown: false }),
            condition: Condvar::new(),
        }
    }

    fn can_acquire(state: &State, priority: i32) -> bool {
        if state.active || state.shutdown {
            return false;
        }
        !state.waiting.iter().any(|(&value, &count)| value < priority && count > 0)
    }

    /// Blocks until this priority may take the turn, or returns `Err(())` if
    /// the scheduler has been shut down while waiting.
    pub fn acquire(&self, priority: i32) -> Result<(), ()> {
        let mut state = self.state.lock().unwrap();
        *state.waiting.entry(priority).or_insert(0) += 1;
        let result = loop {
            if state.shutdown {
                break Err(());
            }
            if Self::can_acquire(&state, priority) {
                state.active = true;
                break Ok(());
            }
            let (guard, _timeout) =
                self.condition.wait_timeout(state, Duration::from_millis(500)).unwrap();
            state = guard;
        };
        if let Some(count) = state.waiting.get_mut(&priority) {
            if *count > 1 {
                *count -= 1;
            } else {
                state.waiting.remove(&priority);
            }
        }
        result
    }

    pub fn release(&self) {
        let mut state = self.state.lock().unwrap();
        state.active = false;
        self.condition.notify_all();
    }

    pub fn shutdown(&self) {
        let mut state = self.state.lock().unwrap();
        state.shutdown = true;
        self.condition.notify_all();
    }

    #[cfg(test)]
    pub fn can_acquire_for_test(&self, priority: i32) -> bool {
        let state = self.state.lock().unwrap();
        Self::can_acquire(&state, priority)
    }

    #[cfg(test)]
    pub fn set_waiting_for_test(&self, priority: i32, count: u32) {
        let mut state = self.state.lock().unwrap();
        state.waiting.clear();
        state.waiting.insert(priority, count);
    }
}

impl Default for TurnScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_priority_wins_over_waiting_prefetch() {
        let scheduler = TurnScheduler::new();
        scheduler.set_waiting_for_test(1, 1);
        assert!(scheduler.can_acquire_for_test(0));
        scheduler.set_waiting_for_test(0, 1);
        assert!(!scheduler.can_acquire_for_test(1));
    }

    #[test]
    fn acquire_then_release_allows_next_caller() {
        let scheduler = TurnScheduler::new();
        scheduler.acquire(0).unwrap();
        scheduler.release();
        scheduler.acquire(1).unwrap();
        scheduler.release();
    }
}
