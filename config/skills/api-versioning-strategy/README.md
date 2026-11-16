# API Versioning Strategy

A Claude Code skill for choosing a versioning scheme and deprecation policy for an API, instead of bumping version numbers inconsistently and breaking consumers with undocumented, unannounced changes.

## Who it helps

- **Architects and tech leads** deciding between URI-path, header, and media-type versioning, and setting the breaking-vs-non-breaking rule the whole team will enforce in code review.
- **Engineering managers** who need a deprecation policy with real dates and telemetry so "we'll remove the old version eventually" doesn't become permanent maintenance debt.
- **Developers** shipping a breaking change to a public or partner API who need a concrete pattern for coexistence, default-version fallback, and sunset headers.

## How it helps

- Frames the version-location decision (URI path vs. header vs. media-type) around visibility-vs-routing-complexity trade-offs, not convention or REST folklore.
- Gives protocol-specific guidance beyond REST: protobuf field-number reservation for gRPC, per-field `@deprecated` for GraphQL — instead of bolting a version header onto everything.
- Walks through a full worked example: a payments API changing a field's type, using date-based version headers, `Deprecation`/`Sunset` response headers, and a usage-telemetry-driven partner off-ramp.
- Cross-references `backward-compatibility-strategy` for the underlying schema-evolution rules and `idempotency-design-for-apis` for what happens to dedup records across a version boundary.

## How to use it

```bash
mkdir -p .claude/skills
cp -r api-versioning-strategy .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/api-versioning-strategy
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/api-versioning-strategy /path/to/your/project/.claude/skills/
```
