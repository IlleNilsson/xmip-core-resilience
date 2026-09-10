#![forbid(unsafe_code)]

//! Retry, timeout, circuit breaker, rate limit, bulkhead and fallback: the
//! policy that names them, and the guard trait the six technologies implement
//! (ADR-0048).

mod guard;

pub use guard::{Attempt, Decision, Failure, Guard, Guarded, execute};

use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    Retryable,
    NonRetryable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub delay: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeoutPolicy {
    pub timeout: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResiliencePolicy {
    pub retry: Option<RetryPolicy>,
    pub timeout: Option<TimeoutPolicy>,
    pub circuit_breaker: bool,
    pub fallback: bool,
    pub rate_limit: Option<u64>,
}

pub trait ResilienceClassifier<E>: Send + Sync {
    fn classify(&self, error: &E) -> FailureKind;
}

pub trait ResilienceExecutor: Send + Sync {
    fn execute<T, E, F>(&self, policy: &ResiliencePolicy, operation: F) -> Result<T, E>
    where
        F: FnMut() -> Result<T, E>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Retries as many times as the policy allows; every other part of the
    /// policy is declared here and enforced elsewhere.
    struct Retrying;

    impl ResilienceExecutor for Retrying {
        fn execute<T, E, F>(&self, policy: &ResiliencePolicy, mut operation: F) -> Result<T, E>
        where
            F: FnMut() -> Result<T, E>,
        {
            let attempts = policy.retry.map_or(1, |retry| retry.max_attempts.max(1));
            let mut last = operation();
            for _ in 1..attempts {
                if last.is_ok() {
                    break;
                }
                last = operation();
            }
            last
        }
    }

    struct ByText;

    impl ResilienceClassifier<String> for ByText {
        fn classify(&self, error: &String) -> FailureKind {
            if error.contains("again") {
                FailureKind::Retryable
            } else {
                FailureKind::NonRetryable
            }
        }
    }

    #[test]
    fn an_executor_honours_the_retry_count() {
        let policy = ResiliencePolicy {
            retry: Some(RetryPolicy {
                max_attempts: 3,
                delay: Duration::ZERO,
            }),
            timeout: Some(TimeoutPolicy {
                timeout: Duration::from_secs(1),
            }),
            circuit_breaker: false,
            fallback: false,
            rate_limit: None,
        };
        let calls = Cell::new(0);
        let outcome: Result<&str, String> = Retrying.execute(&policy, || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err("again".to_string())
            } else {
                Ok("done")
            }
        });
        assert_eq!(outcome, Ok("done"));
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn a_classifier_separates_what_may_be_retried() {
        assert_eq!(
            ByText.classify(&"try again".to_string()),
            FailureKind::Retryable
        );
        assert_eq!(
            ByText.classify(&"malformed".to_string()),
            FailureKind::NonRetryable
        );
    }
}
