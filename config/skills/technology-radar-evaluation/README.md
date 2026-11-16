# Technology Radar Evaluation

A Claude Code skill for running an Adopt/Trial/Assess/Hold technology radar process — evaluating new tools, languages, frameworks, and platforms systematically with evidence-based ring placement, instead of ad hoc, hype-driven adoption.

## Who it helps

- **Architects and tech leads** who need a consistent, evidence-based process for evaluating a proposed technology instead of a one-off enthusiastic pitch.
- **Engineering managers** dealing with uncoordinated tool sprawl across teams who need a governance mechanism connecting radar placement to real adoption decisions.
- **Developers** proposing a new tool who need to know what evidence (a pilot, a spike, production usage) is expected before it can move toward Adopt.

## How it helps

- Defines all four rings precisely and requires evidence, not opinion, behind every placement.
- Time-boxes Trial-ring pilots with named projects and success criteria, so trials produce a decision instead of running forever.
- Connects radar placement to actual governance (what happens when a team wants an exception to a Hold), so the radar isn't just a decorative chart.
- Walks through a full worked example: a bank's quarterly radar covering six real entries across Adopt, Trial, Assess, and Hold, each with a stated rationale and evidence source.

## How to use it

```bash
mkdir -p .claude/skills
cp -r technology-radar-evaluation .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/technology-radar-evaluation
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/technology-radar-evaluation /path/to/your/project/.claude/skills/
```
