# xmip-core-resilience

Provides retry, timeout, circuit breaker, fallback, rate-limit and bulkhead
capabilities. The guards are
[built, not in the assembled service](../../../../doc/architecture/estate-map.md#resilience-guards): the Event
forwarder runs them for the http, amqp and kafka Event wires. The retry
guard is also a Send Port's retry: the runtime's send step asks a Port's
guards after each try that did not send, through `judged`, and the program
starting a node hands in the retry guard its `retry` is built as
([built, in the assembled service](../../../../doc/architecture/estate-map.md#send-port-retry);
ADR-0048, amendment 2026-11-14).
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

`Guard` is the technology's one judgment, asked before and after each
attempt; `execute` asks the guards in order and does what they say, and
`judged` is its asking after an attempt, for a caller that keeps no thread
waiting between attempts, such as the runtime's send step. After
an attempt every guard is asked, whatever the ones before it said, and the
first that does not say proceed decides; before one, every guard is asked
until one refuses or falls back, which decides, and otherwise the attempt
goes after the longest wait asked for (ADR-0048, amendment 2026-11-08). What an
attempt failed with is `xcore::Failure`, the estate's one retryable failure
(ADR-0037, amendment 2026-09-27): whether trying again could help is the
failure's own property, decided where it was met, and a guard reads it.
