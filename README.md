# xmip-core-resilience

Provides retry, timeout, circuit breaker, fallback, rate-limit and bulkhead
capabilities.

## Scope

A native Rust implementation informed by Polly, not a translation of it.
Each guard is a technology beneath this capability (ADR-0048):

```text
Retry   Timeout   Circuit Breaker   Fallback   Rate Limiting   Bulkhead
```

What a Handler reports to these guards, and why it owns no retry loop of its
own, is the estate's rule: `doc/architecture/runtime-model.md`, section 15.
