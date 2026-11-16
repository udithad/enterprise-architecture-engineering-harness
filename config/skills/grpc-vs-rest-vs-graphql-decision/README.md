# gRPC vs. REST vs. GraphQL Decision

A Claude Code skill for picking the right API style per use case — internal service-to-service, first-party multi-client, or public/partner — based on latency, payload-shape flexibility, caching, and streaming needs, instead of defaulting to whichever style is currently fashionable.

## Who it helps

- **Architects and tech leads** designing a new API surface who need a structured way to choose between REST, GraphQL, and gRPC instead of a stylistic debate.
- **Engineering managers** dealing with an existing REST API causing over-/under-fetching pain for mobile clients, or a GraphQL adoption that's stalled on unexpected operational complexity.
- **Developers** building the API who need concrete criteria — caller type, latency, streaming, caching — rather than "gRPC is faster" as the whole justification.

## How it helps

- Starts from caller type (internal, first-party, third-party) since that alone eliminates most of the debate before latency or payload-shape questions even come up.
- Weighs caching, streaming, and typed-contract needs explicitly instead of treating all three styles as interchangeable.
- Walks through a full worked example: a transit platform using gRPC internally, GraphQL for its own multi-client apps, and REST for its public partner API, all stitched together at the gateway.
- Flags the real, often underestimated operational costs of GraphQL (N+1 resolvers, query-cost limiting) and gRPC (debugging ergonomics) rather than presenting either as a free upgrade.

## How to use it

```bash
mkdir -p .claude/skills
cp -r grpc-vs-rest-vs-graphql-decision .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/grpc-vs-rest-vs-graphql-decision
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/grpc-vs-rest-vs-graphql-decision /path/to/your/project/.claude/skills/
```
