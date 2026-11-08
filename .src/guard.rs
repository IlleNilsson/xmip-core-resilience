//! What stands between an attempt and the thing it attempts — the resilience
//! technologies. ADR-0048.
//!
//! An operation is attempted; a guard decides before each attempt whether it
//! may go, must wait, is refused or should be answered by a fallback, and
//! decides after each attempt whether the outcome stands, the attempt is
//! repeated, or the operation is given up. Retry, timeout, circuit breaker,
//! rate limit, bulkhead and fallback are six guards with those two questions
//! each; [`execute`] asks them in order and does what they say.
//!
//! The platform owns the loop; a technology owns one judgment. A guard never
//! runs the operation and never sees its value, only whether it failed and
//! how long it took.

use std::time::{Duration, Instant};

use xcore::Failure;

/// One attempt as a guard sees it: which one it was, how long it took, and
/// whether it failed. The value is not a guard's business.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    /// Counted from one.
    pub number: u32,
    pub elapsed: Duration,
    pub failure: Option<Failure>,
}

impl Attempt {
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        self.failure.is_none()
    }
}

/// What a guard decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Before: the attempt may go. After: the outcome stands.
    Proceed,
    /// Before: go, after this long. After: attempt again, after this long.
    Wait(Duration),
    /// Give up, with the reason the caller will see.
    Refuse(String),
    /// Do not attempt, or do not attempt again: the fallback answers.
    Fallback,
}

/// A resilience technology: one judgment, asked before and after an attempt.
pub trait Guard: Send + Sync {
    /// The manifest leaf: `retry`, `timeout`, `circuit-breaker`, `rate-limit`,
    /// `bulkhead`, `fallback`.
    fn technology(&self) -> &'static str;

    /// May attempt `number` go, counted from one.
    fn before(&self, number: u32) -> Decision;

    /// Does the outcome of this attempt stand.
    fn after(&self, attempt: &Attempt) -> Decision;
}

/// How an execution ended when it did not simply succeed or fail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Guarded<T> {
    /// The operation's own answer.
    Done(T),
    /// A guard said the fallback answers; the caller has it.
    Fallback,
    /// A guard gave up; the reason is the guard's.
    Refused(String),
}

/// Attempt `operation` under `guards`, in the order given, until a guard or
/// the outcome ends it.
///
/// **Before an attempt** every guard is asked, in order, until one refuses
/// or falls back: that one decides, and no guard after it is admitted to an
/// attempt that will not happen. Otherwise the attempt goes after the
/// longest wait any guard asked for, so each guard's wait is kept.
///
/// **After an attempt** every guard is asked, in order, whatever the ones
/// before it said — a bulkhead frees its slot and a circuit breaker counts
/// the failure behind a retry that asks for another try — and the first
/// that does not say proceed decides.
///
/// # Errors
/// The last attempt failed and no guard asked for another.
pub fn execute<T>(
    guards: &[&dyn Guard],
    mut operation: impl FnMut() -> Result<T, Failure>,
) -> Result<Guarded<T>, Failure> {
    let mut number = 1;

    loop {
        match admitted(guards, number) {
            Decision::Proceed => {}
            Decision::Wait(delay) => std::thread::sleep(delay),
            Decision::Refuse(reason) => return Ok(Guarded::Refused(reason)),
            Decision::Fallback => return Ok(Guarded::Fallback),
        }

        let started = Instant::now();
        let result = operation();
        let attempt = Attempt {
            number,
            elapsed: started.elapsed(),
            failure: result.as_ref().err().cloned(),
        };

        match judged(guards, &attempt) {
            Decision::Proceed => return result.map(Guarded::Done),
            Decision::Wait(delay) => {
                std::thread::sleep(delay);
                number += 1;
            }
            Decision::Refuse(reason) => return Ok(Guarded::Refused(reason)),
            Decision::Fallback => return Ok(Guarded::Fallback),
        }
    }
}

/// Every guard asked before attempt `number`, in order, until one refuses
/// or falls back, which decides; otherwise the longest wait, or proceed.
fn admitted(guards: &[&dyn Guard], number: u32) -> Decision {
    let mut longest: Option<Duration> = None;
    for guard in guards {
        match guard.before(number) {
            Decision::Proceed => {}
            Decision::Wait(delay) => longest = longest.max(Some(delay)),
            ended @ (Decision::Refuse(_) | Decision::Fallback) => return ended,
        }
    }
    longest.map_or(Decision::Proceed, Decision::Wait)
}

/// Every guard asked after `attempt`, in order, each one whatever the
/// others said; the first that does not say proceed decides.
fn judged(guards: &[&dyn Guard], attempt: &Attempt) -> Decision {
    let mut decided = Decision::Proceed;
    for guard in guards {
        let decision = guard.after(attempt);
        if decided == Decision::Proceed {
            decided = decision;
        }
    }
    decided
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    /// Tries again on a retryable failure, up to a count.
    struct Again(u32);

    impl Guard for Again {
        fn technology(&self) -> &'static str {
            "retry"
        }

        fn before(&self, _: u32) -> Decision {
            Decision::Proceed
        }

        fn after(&self, attempt: &Attempt) -> Decision {
            match &attempt.failure {
                Some(failure) if failure.retryable && attempt.number < self.0 => {
                    Decision::Wait(Duration::ZERO)
                }
                _ => Decision::Proceed,
            }
        }
    }

    /// Refuses everything before it starts.
    struct Shut;

    impl Guard for Shut {
        fn technology(&self) -> &'static str {
            "circuit-breaker"
        }

        fn before(&self, _: u32) -> Decision {
            Decision::Refuse("open".into())
        }

        fn after(&self, _: &Attempt) -> Decision {
            Decision::Proceed
        }
    }

    /// Answers a failure with the fallback.
    struct Instead;

    impl Guard for Instead {
        fn technology(&self) -> &'static str {
            "fallback"
        }

        fn before(&self, _: u32) -> Decision {
            Decision::Proceed
        }

        fn after(&self, attempt: &Attempt) -> Decision {
            if attempt.succeeded() {
                Decision::Proceed
            } else {
                Decision::Fallback
            }
        }
    }

    #[test]
    fn a_retry_guard_repeats_a_retryable_failure_and_the_value_comes_through() {
        let calls = Cell::new(0);
        let guards: [&dyn Guard; 1] = [&Again(3)];
        let outcome = execute(&guards, || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(Failure::retryable("again"))
            } else {
                Ok("done")
            }
        });
        assert_eq!(outcome, Ok(Guarded::Done("done")));
        assert_eq!(calls.get(), 3);

        let permanent: Result<Guarded<()>, Failure> =
            execute(&guards, || Err(Failure::permanent("broken")));
        assert_eq!(permanent, Err(Failure::permanent("broken")));
    }

    #[test]
    fn the_first_guard_that_does_not_proceed_decides_in_order() {
        let calls = Cell::new(0);
        let guards: [&dyn Guard; 2] = [&Shut, &Again(3)];
        let refused = execute(&guards, || {
            calls.set(calls.get() + 1);
            Ok(1)
        });
        assert_eq!(refused, Ok(Guarded::Refused("open".into())));
        assert_eq!(calls.get(), 0, "refused before any attempt");

        let guards: [&dyn Guard; 2] = [&Again(2), &Instead];
        let fell_back: Result<Guarded<i32>, Failure> = execute(&guards, || {
            calls.set(calls.get() + 1);
            Err(Failure::retryable("again"))
        });
        assert_eq!(fell_back, Ok(Guarded::Fallback));
        assert_eq!(calls.get(), 2, "retry went first and ran out");
    }

    /// A slot taken before each attempt and freed after it, refused while
    /// taken: a bulkhead of one.
    #[derive(Default)]
    struct Slot {
        taken: AtomicBool,
        asked_after: AtomicU32,
    }

    impl Slot {
        fn taken(&self) -> bool {
            self.taken.load(Ordering::Relaxed)
        }
    }

    impl Guard for Slot {
        fn technology(&self) -> &'static str {
            "bulkhead"
        }

        fn before(&self, _: u32) -> Decision {
            if self.taken.swap(true, Ordering::Relaxed) {
                Decision::Refuse("the slot is taken".into())
            } else {
                Decision::Proceed
            }
        }

        fn after(&self, _: &Attempt) -> Decision {
            self.taken.store(false, Ordering::Relaxed);
            self.asked_after.fetch_add(1, Ordering::Relaxed);
            Decision::Proceed
        }
    }

    /// Waits before every attempt.
    struct Paced(Duration);

    impl Guard for Paced {
        fn technology(&self) -> &'static str {
            "rate-limit"
        }

        fn before(&self, _: u32) -> Decision {
            Decision::Wait(self.0)
        }

        fn after(&self, _: &Attempt) -> Decision {
            Decision::Proceed
        }
    }

    #[test]
    fn a_guard_behind_a_retry_sees_every_attempt_it_asks_again_after() {
        // The retry's wait ended the asking: the slot behind it was never
        // freed, and the second attempt was refused for it.
        let slot = Slot::default();
        let guards: [&dyn Guard; 2] = [&Again(3), &slot];
        let calls = Cell::new(0);
        let outcome = execute(&guards, || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(Failure::retryable("again"))
            } else {
                Ok("done")
            }
        });
        assert_eq!(outcome, Ok(Guarded::Done("done")));
        assert_eq!(
            (calls.get(), slot.asked_after.load(Ordering::Relaxed)),
            (3, 3)
        );
        assert!(!slot.taken(), "freed after the last attempt");
    }

    #[test]
    fn a_guard_behind_a_wait_is_asked_before_the_attempt_and_the_longest_wait_kept() {
        // The wait ended the asking: the slot behind it was never taken
        // before the attempt, and freed after it all the same.
        let slot = Slot::default();
        let guards: [&dyn Guard; 3] = [
            &Paced(Duration::from_millis(1)),
            &slot,
            &Paced(Duration::from_millis(3)),
        ];
        let started = Instant::now();
        let outcome = execute(&guards, || {
            assert!(slot.taken(), "the slot is taken for the attempt");
            Ok(1)
        });
        assert_eq!(outcome, Ok(Guarded::Done(1)));
        assert!(started.elapsed() >= Duration::from_millis(3), "the longest");
        assert_eq!(
            admitted(&[&Paced(Duration::ZERO), &Shut], 1),
            Decision::Refuse("open".into())
        );
    }

    #[test]
    fn an_attempt_reports_its_number_its_failure_and_that_it_took_time() {
        struct Watching(std::sync::Mutex<Vec<Attempt>>);
        impl Guard for Watching {
            fn technology(&self) -> &'static str {
                "timeout"
            }
            fn before(&self, _: u32) -> Decision {
                Decision::Proceed
            }
            fn after(&self, attempt: &Attempt) -> Decision {
                self.0.lock().expect("no poison").push(attempt.clone());
                Decision::Proceed
            }
        }
        let watching = Watching(std::sync::Mutex::new(Vec::new()));
        let guards: [&dyn Guard; 1] = [&watching];
        let _: Result<Guarded<()>, Failure> = execute(&guards, || Err(Failure::permanent("no")));
        let attempts = watching.0.lock().expect("no poison");
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].number, 1);
        assert!(!attempts[0].succeeded());
        assert_eq!(
            attempts[0]
                .failure
                .as_ref()
                .map(|failure| failure.message.as_str()),
            Some("no")
        );
    }
}
