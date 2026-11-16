---
name: hexagonal-architecture-design
description: "Isolates business logic behind ports (interfaces) and adapters (implementations) so the domain stays independent of frameworks, databases, and UI technology. Use when you need the core logic testable without a database or when you expect to swap a driving/driven technology (e.g., REST to gRPC, Postgres to DynamoDB) without touching business rules."
---

# Hexagonal Architecture Design (Ports & Adapters)

Keep the application core (domain logic and use cases) at the center, talking only to abstract "ports," while concrete "adapters" plug in on both sides — driving adapters that call into the core (HTTP, CLI, message consumers) and driven adapters the core calls out to (database, external APIs, file system).

## When to use this

- The domain logic needs to be unit-tested without spinning up a real database, message broker, or HTTP server.
- You anticipate swapping a technology (e.g., moving from REST to gRPC, from Postgres to DynamoDB, from Kafka to SQS) and want that change to touch only one adapter, not the core.
- Multiple entry points need to trigger the same use case (HTTP API, CLI, scheduled job, message consumer) and the logic must not be duplicated across them.
- A team keeps writing integration tests for logic that should be a fast unit test, because the business rule is entangled with a database call.
- You're building a library or core engine that will be embedded in different runtime contexts (a web service today, a batch job tomorrow).

## Workflow

1. **Identify the application core.** List the use cases (e.g., `RegisterUser`, `ReserveInventory`, `GenerateInvoice`) — these become the core's public API.
2. **Define driving ports** — interfaces the outside world calls to invoke a use case (often just the use-case classes/functions themselves, or an explicit interface if you want to decouple from a concrete implementation).
3. **Define driven ports** — interfaces the core calls out through, one per external dependency the core needs (e.g., `UserRepository`, `EmailSender`, `ClockPort`). These are owned and defined by the core, not by the adapter.
4. **Build driving adapters** — the technology-specific code that receives external input and calls a driving port: an HTTP controller, a gRPC service handler, a CLI command, a Kafka consumer. Each translates its transport's request format into the core's input type.
5. **Build driven adapters** — technology-specific implementations of driven ports: a Postgres-backed `UserRepository`, an SES-backed `EmailSender`, a system-clock `ClockPort`, and a fake/in-memory version of each for tests.
6. **Wire adapters to ports at the composition root** (application startup) via dependency injection — the core never constructs an adapter itself.
7. **Test the core with fake driven adapters** (in-memory repository, fake clock, fake email sender) — this should be the majority of your test suite, running in milliseconds with no I/O.
8. **Add a small number of adapter-level integration tests** to verify each real adapter actually satisfies its port's contract (e.g., a contract test suite run against both the real Postgres adapter and the in-memory fake).
9. **Keep ports narrow and use-case-shaped**, not thin wrappers around a library's full API (avoid `interface UserRepository { Connection getConnection(); }` — that just re-exposes the database).

## Example

Scenario: a subscription-billing core needs to work identically whether triggered by an HTTP API or a nightly batch renewal job, and must be testable without a real payment processor.

```
billing-core/
├── core/
│   ├── renew_subscription.py     # use case: the driving port's implementation
│   ├── ports/
│   │   ├── subscription_repository.py   # driven port (interface)
│   │   ├── payment_processor.py         # driven port (interface)
│   │   └── clock.py                     # driven port (interface)
├── adapters/
│   ├── driving/
│   │   ├── http_api.py            # FastAPI route -> calls RenewSubscription
│   │   └── batch_job.py           # cron entry point -> calls RenewSubscription for all due subs
│   └── driven/
│       ├── postgres_subscription_repository.py
│       ├── stripe_payment_processor.py
│       ├── system_clock.py
│       └── fakes/
│           ├── in_memory_subscription_repository.py
│           ├── fake_payment_processor.py
│           └── fixed_clock.py
```

```python
# core/renew_subscription.py — the core, depends only on ports
class RenewSubscription:
    def __init__(self, subs: SubscriptionRepository, payments: PaymentProcessor, clock: Clock):
        self._subs, self._payments, self._clock = subs, payments, clock

    def execute(self, subscription_id: str) -> RenewalResult:
        sub = self._subs.get(subscription_id)
        if not sub.is_due(self._clock.now()):
            return RenewalResult.not_due(subscription_id)
        charge = self._payments.charge(sub.customer_id, sub.plan.price)
        if charge.succeeded:
            sub.extend_period()
            self._subs.save(sub)
            return RenewalResult.renewed(subscription_id)
        return RenewalResult.failed(subscription_id, charge.error)
```

Unit test uses `InMemorySubscriptionRepository`, `FakePaymentProcessor`, and `FixedClock` — no network, no database, runs in microseconds. The HTTP adapter and the batch job adapter both call `RenewSubscription.execute` and share 100% of the business logic.

## Common pitfalls

- **Ports defined by the adapter's shape, not the core's need.** If a port interface mirrors a specific SDK's method signatures, you haven't actually decoupled — define ports from what the *core* needs, not what a library offers.
- **Skipping the fakes.** The main payoff of hexagonal architecture is fast, dependency-free core tests; if every test still spins up Testcontainers, you haven't captured the benefit.
- **Too many ports for trivial dependencies.** Not every function call needs to be a port — reserve ports for boundaries you genuinely expect to swap or need to fake in tests (I/O, external services, time, randomness).
- **Adapters containing business logic.** If the HTTP controller computes a discount before calling the core, that logic just leaked outside the hexagon; push it back into the core.
- **No contract tests for driven adapters.** Without a shared contract-test suite run against both the real and fake implementations, they can silently drift apart and hide bugs until production.
- **Confusing this with "just add an interface everywhere."** Ports need real semantic ownership by the core (defined by what it needs), not mechanical interface-extraction from every class.

## Output

A restructured core/adapters codebase (or module boundary) with: a `core/` package containing use cases and port interfaces with zero framework imports; `adapters/driving/` and `adapters/driven/` packages implementing those ports; at least one in-memory fake per driven port; a composition root wiring real adapters in production and fakes in tests; and a documented list of ports with what each represents.
