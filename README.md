# xmip-core-resilience
Provides retry, timeout, circuit breaker, fallback and rate-limit capabilities.

## Scope

A native Rust implementation informed by Polly, not a translation of it.
Each guard is a technology beneath this capability (ADR-0048):

```text
Retry   Timeout   Circuit Breaker   Fallback   Rate Limiting   Bulkhead
```

Handlers report Success, Retryable Failure or Non-retryable Failure and own no
retry loop; the rule is `doc/architecture/runtime-model.md`, section 15.
