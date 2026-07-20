#![forbid(unsafe_code)]

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
