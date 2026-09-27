#![forbid(unsafe_code)]

//! Retry, timeout, circuit breaker, rate limit, bulkhead and fallback: the
//! guard trait the six technologies implement, and the loop that asks them
//! (ADR-0048). What an attempt failed with is `xcore::Failure`, the one
//! retryable failure: whether it is worth trying again is the failure's own
//! property, decided where it was met.

mod guard;

pub use guard::{Attempt, Decision, Guard, Guarded, execute};
