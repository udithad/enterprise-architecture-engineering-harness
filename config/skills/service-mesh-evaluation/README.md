# Service Mesh Evaluation

A Claude Code skill for deciding whether a service mesh is worth its operational cost for a given system — grounded in measured east-west traffic pain points rather than infrastructure trend-following.

## Who it helps

- **Architects and tech leads** who need a defensible yes/no when someone proposes adopting Istio, Linkerd, or a cloud-managed mesh, based on real symptoms rather than popularity.
- **Engineering managers** weighing the ongoing control-plane ownership cost (a named team, upgrades, CVE response) against the security/consistency problem a mesh would solve.
- **Developers** currently hand-rolling retries, timeouts, or mTLS per service who need to know whether mesh-level infrastructure or a simpler in-app library is the right fix.

## How it helps

- Opens with a hard threshold check (service count, team size, measured pain points) before evaluating any specific product, to head off adopting a mesh as a reflexive best practice.
- Breaks the mesh's bundled capabilities (mTLS, retries/circuit-breaking, traffic shifting, telemetry) apart so each can be evaluated against a lighter alternative on its own merit.
- Walks through a full worked example: a 22-service healthcare platform with a real compliance-driven mTLS need, choosing Linkerd over Istio for lower operational overhead, with a staged rollout plan and measured latency budget.
- Draws an explicit line between mesh-owned transport concerns and application-owned business logic, cross-referencing `circuit-breaker-pattern-design` and `api-gateway-design` for where those concerns actually belong.

## How to use it

```bash
mkdir -p .claude/skills
cp -r service-mesh-evaluation .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/service-mesh-evaluation
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/service-mesh-evaluation /path/to/your/project/.claude/skills/
```
