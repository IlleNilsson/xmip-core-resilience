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
//! The platform owns the loop; a technology owns one judgement. A guard never
//! runs the operation and never sees its value, only whether it failed and
//! how long it took.

use std::time::{Duration, Instant};

use crate::FailureKind;

/// Why an attempt failed, and whether trying again could change that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    pub reason: String,
}

impl Failure {
    #[must_use]
    pub fn retryable(reason: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::Retryable,
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn permanent(reason: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::NonRetryable,
            reason: reason.into(),
        }
    }

    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(self.kind, FailureKind::Retryable)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for Failure {}

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

/// A resilience technology: one judgement, asked before and after an attempt.
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
/// the outcome ends it. Every guard is asked before every attempt and after
/// it; the first that does not say proceed decides.
///
/// # Errors
/// The last attempt failed and no guard asked for another.
pub fn execute<T>(
    guards: &[&dyn Guard],
    mut operation: impl FnMut() -> Result<T, Failure>,
) -> Result<Guarded<T>, Failure> {
    let mut number = 1;

    loop {
        match first_not_proceeding(guards.iter().map(|guard| guard.before(number))) {
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

        match first_not_proceeding(guards.iter().map(|guard| guard.after(&attempt))) {
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

fn first_not_proceeding(mut decisions: impl Iterator<Item = Decision>) -> Decision {
    decisions
        .find(|decision| *decision != Decision::Proceed)
        .unwrap_or(Decision::Proceed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

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
                Some(failure) if failure.is_retryable() && attempt.number < self.0 => {
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
            attempts[0].failure.as_ref().map(ToString::to_string),
            Some("no".into())
        );
    }
}
