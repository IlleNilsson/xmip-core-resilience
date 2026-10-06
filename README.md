# xmip-core-resilience

Provides retry, timeout, circuit breaker, fallback, rate-limit and bulkhead
capabilities. The guards are
[built, not in the assembled service](../../../../doc/architecture/estate-map.md#resilience-guards): the Event
forwarder runs them for the http, amqp and kafka Event wires, only tests
build one, and a Message send does not ask them — its Send Port's own retry
is what it has ([built, in the assembled service](../../../../doc/architecture/estate-map.md#send-port-retry)).
Guards an operator configures on a Send Port Group, Send Port and Send
Location, and a timeout that interrupts an attempt, are
[decided, not built](../../../../doc/architecture/estate-map.md#send-resilience) (ADR-0048, amendments
2026-10-06); the timeout here judges an attempt after it ended.

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
