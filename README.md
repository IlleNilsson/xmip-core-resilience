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

## The surface

`Guard` is the technology's one judgement, asked before and after each
attempt; `execute` asks the guards in order and does what they say. What an
attempt failed with is `xcore::Failure`, the estate's one retryable failure
(ADR-0037, amendment 2026-09-27): whether trying again could help is the
failure's own property, decided where it was met, and a guard reads it.
