---
name: layered-architecture-design
description: "Structures a system into presentation, application, domain, and infrastructure layers with a strict, enforceable dependency direction. Use when a codebase is becoming tangled — UI code touching SQL, business rules scattered across controllers — and needs a clear separation of concerns before it grows further."
---

# Layered Architecture Design

Organize a system into horizontal layers — each with a single responsibility and a one-way dependency rule — so business logic stays testable and swappable independent of UI or infrastructure changes.

## When to use this

- A new service or module is being scaffolded and needs a starting internal structure.
- Business logic is duplicated across controllers, background jobs, and CLI entry points because there's no shared layer to hold it.
- SQL queries or HTTP client calls are scattered directly inside request handlers, making unit testing require a real database.
- A team is onboarding engineers who need a predictable place to find "where does X belong."
- Code review keeps flagging layering violations (e.g., a domain class importing a web framework type) but there's no written rule to point to.
- Preparing to extract a module into its own service later — clean layering makes that extraction mechanical rather than exploratory.

## Workflow

1. **Define the layers for this system.** The classic four are usually enough:
   - **Presentation** — HTTP controllers, CLI commands, message consumers: translates external requests into calls on the application layer, and results back into responses.
   - **Application** — use-case orchestration: coordinates domain objects and infrastructure ports to fulfill one use case (e.g., `PlaceOrder`, `CancelSubscription`). Contains no business rules itself.
   - **Domain** — entities, value objects, domain services: the business rules, invariants, and calculations. No framework or I/O imports allowed.
   - **Infrastructure** — database repositories, external API clients, message publishers: implements interfaces defined by the domain/application layers.
2. **Write the dependency rule down explicitly**: dependencies point inward only — Presentation → Application → Domain, and Infrastructure implements interfaces defined in Domain/Application (dependency inversion), never the reverse. Domain never imports Infrastructure or Presentation.
3. **Place code by responsibility, not by technical type.** Avoid grouping everything by "controllers/", "models/", "utils/" alone; nest per-feature within each layer if the codebase is large (e.g., `application/orders/place_order.py`).
4. **Define the boundary interfaces first** for anything Domain/Application needs from Infrastructure (e.g., `OrderRepository`, `PaymentGateway`) as abstract interfaces/protocols owned by the inner layer.
5. **Implement infrastructure adapters** against those interfaces, injected at the composition root (main/bootstrap) rather than constructed inside domain or application code.
6. **Add an automated layering check** (see `architecture-fitness-function-design` skill) — a static-analysis rule (e.g., import-linter for Python, ArchUnit for Java, a custom clang-tidy check for C++) that fails the build if an inner layer imports an outer one.
7. **Review for leakage**: an ORM entity used directly as a domain object, a framework annotation on a domain class, an HTTP status code returned from application-layer code — these are all common signs a layer boundary has been crossed.
8. **Document the layering** in a short `ARCHITECTURE.md` at the repo root: one paragraph per layer, the dependency rule, and where new code of each kind should go.

## Example

Scenario: an order-management service currently has all logic in Flask route handlers, directly calling SQLAlchemy and a payment SDK.

Target structure:

```
order_service/
├── presentation/
│   └── http/order_routes.py       # Flask blueprint; parses request, calls application layer
├── application/
│   └── place_order.py             # PlaceOrderUseCase: orchestrates domain + ports
├── domain/
│   ├── order.py                   # Order entity: validates line items, computes total
│   ├── order_repository.py        # abstract interface (Protocol/ABC)
│   └── payment_gateway.py         # abstract interface
└── infrastructure/
    ├── sqlalchemy_order_repository.py  # implements OrderRepository
    └── stripe_payment_gateway.py       # implements PaymentGateway
```

```python
# application/place_order.py — depends only on domain interfaces
class PlaceOrderUseCase:
    def __init__(self, orders: OrderRepository, payments: PaymentGateway):
        self._orders = orders
        self._payments = payments

    def execute(self, customer_id: str, items: list[LineItem]) -> Order:
        order = Order.create(customer_id, items)   # domain enforces invariants
        self._payments.charge(order.total, customer_id)
        self._orders.save(order)
        return order
```

```python
# presentation/http/order_routes.py — translates HTTP <-> use case
@app.post("/orders")
def create_order():
    payload = request.get_json()
    order = place_order_use_case.execute(payload["customer_id"], parse_items(payload["items"]))
    return jsonify(order.to_dict()), 201
```

The Flask app, SQLAlchemy session, and Stripe client are all wired together only in `main.py` (the composition root), so `domain/order.py` never imports Flask, SQLAlchemy, or `stripe`.

## Common pitfalls

- **Anemic domain layer.** If `domain/` only holds data classes with getters/setters and all logic lives in `application/`, you've built a layered structure without actually separating business rules from orchestration — watch for this drift over time.
- **ORM models doubling as domain entities.** Coupling persistence annotations to domain classes makes the domain layer secretly depend on infrastructure. Keep a mapping layer, even if it's some duplication.
- **Application layer accreting business rules.** Once a use case starts doing `if order.total > 1000: apply_discount()`, that rule belongs in the domain, not the orchestration layer.
- **No automated enforcement.** Written rules erode within a few sprints without a static check in CI; add one from day one, even a simple import-direction linter.
- **Over-layering a small service.** A 200-line CRUD microservice doesn't need four layers; use judgment — apply full layering where business complexity justifies it.
- **Presentation-layer logic leaking into domain via shared DTOs.** Don't reuse HTTP request/response schemas as domain objects; convert at the boundary.

## Output

An `ARCHITECTURE.md` (or equivalent) documenting the layers, the dependency rule, and directory conventions, plus the actual directory/module restructuring in the codebase, plus a CI-enforced fitness function (linter rule or custom script) that fails the build on a layering violation.
