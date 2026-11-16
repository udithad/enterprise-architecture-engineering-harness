---
name: observability-architecture-design
description: "Architects logging, metrics, and tracing as a coherent system so engineers can diagnose unfamiliar production failures quickly, instead of bolting on ad hoc log statements after each incident. Use when designing or auditing a system's observability before it goes to production, or after an incident revealed a diagnostic blind spot."
---

# Observability Architecture Design

Design logging, metrics, and distributed tracing as one coherent system — with consistent correlation IDs, deliberate cardinality control, and defined alerting thresholds — so an engineer unfamiliar with a specific code path can still diagnose a production failure quickly, instead of observability being an afterthought bolted on incident by incident.

## When to use this

- A new service or system is being designed and observability hasn't been planned beyond "we'll add some logs."
- A recent incident took too long to diagnose because a critical signal (a trace across services, a metric on a specific failure mode) simply didn't exist.
- Logs exist but are unstructured, inconsistent, or don't propagate a correlation/trace ID across service boundaries, making cross-service debugging (`log-correlation-across-services`) painful.
- A metrics system exists but is either too sparse (no visibility into what's actually failing) or too high-cardinality (exploding cost, or timing out on query).
- Preparing an `architecture-review-board-prep` package and observability needs to be shown as a first-class design concern, not an implementation detail.

## Workflow

1. **Define the three pillars' distinct jobs before choosing tools**: metrics answer "is something wrong and how bad" (aggregate, cheap, good for alerting); logs answer "what exactly happened" (detailed, per-event, more expensive); traces answer "where in a multi-service call chain did it happen" (causal, cross-service). Don't try to make one pillar do all three jobs.
2. **Design a consistent correlation ID scheme first** — a single trace/request ID generated at the edge, propagated through every service hop (in headers, in async message metadata, in log context) so a single user-facing failure can be traced end to end across `log-correlation-across-services` and `distributed-tracing-debugging`.
3. **Choose structured logging with a fixed schema** (JSON or equivalent) over free-text logs — fields like `timestamp`, `service`, `trace_id`, `level`, `message`, plus domain-specific fields — so logs are queryable, not just greppable.
4. **Define metrics deliberately per service, not by reflex-instrumenting everything**: the RED method (Rate, Errors, Duration) for request-driven services, the USE method (Utilization, Saturation, Errors) for resources, and business-specific metrics (orders/sec, queue depth) that map to what actually matters for this system.
5. **Control cardinality explicitly** — never put unbounded values (user ID, raw URL with path params, timestamps) directly into a metric label; this is the single most common cause of a metrics system becoming slow or unaffordable. Use logs or traces for high-cardinality data instead.
6. **Instrument traces at every service boundary and every externally-called dependency** (DB, cache, downstream API), including timing, so a trace visually shows where time was spent across a distributed call, not just that a call was slow somewhere.
7. **Define alerting on symptoms, not causes** — alert on "p99 latency exceeded SLO" or "error rate above threshold" (user-visible symptoms), and use logs/traces for root-causing once alerted, rather than alerting on every internal cause which produces noise and alert fatigue.
8. **Set explicit log/metric/trace retention and sampling policy up front** — full-fidelity tracing on 100% of traffic is often cost-prohibitive at scale; define a sampling strategy (e.g., 100% of errors, 1% of successes) deliberately rather than discovering the cost problem after rollout.
9. **Design dashboards around a small number of "is the system healthy" views plus deeper drill-down views** — a wall of 200 undifferentiated graphs is not usable during an incident; a small golden-signals dashboard plus linked drill-downs is.
10. **Validate observability during a game day or chaos exercise**, not just in normal operation — inject a real failure and check whether the existing signals actually let someone diagnose it within the target time; gaps found here are far cheaper than gaps found during a real incident.

## Example

Scenario: `RouteWise`, a logistics platform, had a production incident where a delivery-estimate API was returning wrong results intermittently. Diagnosis took four hours because logs from the three services involved (routing service, pricing service, traffic-data service) had no shared correlation ID, and the only metric available was an aggregate error rate that didn't distinguish this failure mode from unrelated errors.

Redesign:
- **Correlation ID**: a `X-Request-ID` generated at the API gateway, propagated via header on every synchronous call and via message attribute on every async event, included in every log line and every span.
- **Structured logging**: all three services migrated to JSON logs with a fixed schema (`trace_id`, `service`, `level`, `event`, plus domain fields like `route_id`); free-text `printf`-style logs removed from the request path.
- **Metrics**: RED metrics added per endpoint (`http_requests_total`, `http_request_duration_seconds`, `http_errors_total`, each labeled by `service` and `endpoint` — deliberately not by `route_id`, which is unbounded), plus a business metric `estimate_mismatch_total` specifically counting the failure mode from the incident, so it's directly visible going forward instead of hiding inside a generic error rate.
- **Tracing**: OpenTelemetry instrumentation added at every service boundary and at the call to the external traffic-data provider, so a single trace now shows the full path of one delivery-estimate request across all three services plus the external call's latency.
- **Alerting**: a new symptom-based alert on `estimate_mismatch_total` rate exceeding a threshold, routed to the owning team, replacing reliance on the generic error-rate alert that hadn't distinguished this case.
- **Validation**: a follow-up game day deliberately reproduced a similar traffic-data staleness failure; the new trace and metric let an on-call engineer unfamiliar with the original incident localize the cause in under 10 minutes.

## Common pitfalls

- **No shared correlation/trace ID across services**, making cross-service incidents require manually correlating timestamps across disjoint logs.
- **Unbounded label cardinality in metrics** (user IDs, raw paths, request IDs as labels), silently degrading or blowing up the cost of the metrics backend.
- **Free-text, inconsistent logs** that can't be queried structurally, only grepped, and differently in every service.
- **Alerting on internal causes instead of user-visible symptoms**, producing alert fatigue that trains on-call engineers to ignore pages.
- **Instrumenting reflexively everywhere** without asking what question each signal answers, producing high volume and low signal.
- **No sampling/retention strategy decided up front**, discovering the cost or performance impact of full tracing only after it's already expensive in production.
- **Never testing observability under a real failure**, so gaps are discovered during an actual incident instead of a rehearsal.

## Output

An observability architecture doc (or section of the system's architecture doc) specifying: the correlation ID propagation scheme, the structured logging schema, the metrics taken per service (with cardinality rules), the tracing instrumentation points and sampling policy, symptom-based alert definitions with routing, and the dashboard set (golden-signals + drill-down).
