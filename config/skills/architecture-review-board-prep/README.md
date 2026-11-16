# Architecture Review Board Prep

A Claude Code skill for packaging a proposed design into what an architecture review board actually needs to decide fast: a tight problem statement, the real options considered, right-altitude diagrams, and an explicit risk register — instead of a rambling deck the board has to reverse-engineer.

## Who it helps

- **Architects and tech leads** presenting a cross-team or high-blast-radius design who want a decision in one sitting instead of a tabled "come back with more detail."
- **Engineering managers** who need governance sign-off tied to budget, compliance, or cross-org visibility, and want the packet to pre-answer the questions every board asks.
- **Developers** new to formal review processes who need a template for what belongs in an ARB packet versus a team design doc.

## How it helps

- Structures the packet around what boards actually evaluate: problem driver, real alternatives (not a strawman), explicit recommendation, and pre-answered cost/security/ops questions.
- Pushes diagrams to the right altitude — system context and C4 container level — instead of burying the decision under component-level detail.
- Walks through a full worked example: a service-mesh ARB packet for a 40-service platform, including a risk register with owners and a conditional-approval outcome captured back into the ADR.
- Flags the pitfalls that waste a review slot — strawman options, no written decision afterward, presenting a design as risk-free.

## How to use it

```bash
mkdir -p .claude/skills
cp -r architecture-review-board-prep .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/architecture-review-board-prep
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/architecture-review-board-prep /path/to/your/project/.claude/skills/
```
