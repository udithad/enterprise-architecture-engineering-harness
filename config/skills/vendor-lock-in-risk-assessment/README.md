# Vendor Lock-In Risk Assessment

A Claude Code skill for quantifying switching costs and defining an exit strategy before committing to a vendor, cloud-specific service, or proprietary technology, so lock-in is a knowingly accepted trade-off rather than a surprise discovered at renewal time.

## Who it helps

- **Architects and tech leads** deciding whether to adopt a vendor-specific service, who need switching cost estimated in concrete engineer-weeks rather than a vague "it would be hard to leave."
- **Engineering managers** heading into a vendor renewal negotiation who need to know their actual switching cost to negotiate from an informed position.
- **Developers** designing an integration who need guidance on when a lightweight abstraction seam is worth the upfront cost versus when it's unnecessary overhead.

## How it helps

- Separates switching cost (a number that can be estimated) from lock-in risk (that cost weighed against vendor stability and likelihood of needing to leave).
- Pushes back on "avoid all lock-in" as a blanket rule, showing when accepting lock-in is the economically correct trade-off.
- Insists on checking data portability and export capability before signing a contract, not after.
- Walks through a full worked example: a media analytics platform's decision to accept lock-in to a proprietary streaming service, with a quantified switching-cost estimate and a cheap abstraction-seam mitigation.

## How to use it

```bash
mkdir -p .claude/skills
cp -r vendor-lock-in-risk-assessment .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/vendor-lock-in-risk-assessment
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/vendor-lock-in-risk-assessment /path/to/your/project/.claude/skills/
```
