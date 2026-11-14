# Organization policies

Place enterprise or project policy documents (`*.json`, `PolicyDocument` format) here.
They are loaded at startup in addition to the built-in structural baseline
(`eaeh.baseline`, see `crates/eaeh-governance/src/policy.rs`).

EAEH ships **no** enterprise policies: enterprise standards must be supplied by the
organization (action item A-09). Evaluation is default-deny and deny-overrides, so a
project-level policy can never weaken an enterprise-level rule.
