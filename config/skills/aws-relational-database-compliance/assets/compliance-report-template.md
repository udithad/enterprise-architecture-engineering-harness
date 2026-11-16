# Relational database compliance report template

Fill every row from evidence. Use only these statuses: `COMPLIANT (evidence)`, `VIOLATION`, `NOT VERIFIED`. A missing evidence item is `NOT VERIFIED`, never `COMPLIANT`.

**Target:** <path or description>   **Overall:** NON-COMPLIANT | COMPLIANT (all mandatory rules evidenced) | INCONCLUSIVE

## Checks executed

| Command | Executed? | Result (verbatim) |
|---|---|---|
| terraform fmt / validate / plan | yes / no (reason) | |
| validator.py | yes / no | |
| AWS CLI evidence commands | yes / no (reason) | |

## Mandatory guardrails

| # | Guardrail | Source | Status | Evidence (file, line, command output) | Remediation (control-preserving) |
|---|---|---|---|---|---|
| 1 | CMEK storage encryption | CSS_1 s2 | | | |
| 3 | `storage_encrypted` true, non-empty `kms_key_id` | CSS_1 s4, s5 | | | |
| 4 | Backups, replicas, restores inherit customer-managed key | CSS_1 s2, s5 | | | |
| 6 | AWS only | CGS_1 s1, s6 | | | |
| 8 | Approved regions | CGS_1 s2 | | | |
| 9 | Private subnets, not publicly accessible | CGS_1 s2, s6 | | | |
| 10 | Production Multi-AZ | CGS_1 s2 | | | |
| 11 | Production deletion protection; retention 7-35 days | CGS_1 s5 | | | |
| 12 | Four mandatory tags | CGS_1 s4 | | | |
| 13 | gp3 + autoscaling; no io1/io2 in dev/stage; non-prod cap | CGS_1 s3, s6 | | | |
| 14 | Dev scheduled shutdown supported | CGS_1 s5 | NOT VERIFIED (no mechanism specified) | | |
| 15 | Approved engine (PostgreSQL 15+) | EAS_1 s2 | | | |
| 16-18 | Database-per-service, migrations, no dual writes/2PC | EAS_1 s4, s5 | | | |

## Recommendations (warnings, not violations)

| Item | Source | Applies? | Decision and reason |
|---|---|---|---|
| Serverless v2 for variable or low-throughput workloads | CGS_1 s3 | | |

## Unresolved conflicts, assumptions, placeholders

- <list; escalate conflicts to a human>
