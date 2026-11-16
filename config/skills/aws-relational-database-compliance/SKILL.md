+++
# EAEH skill directory. This TOML front matter is the manifest; the Markdown below is the
# main instruction text. Every file in this folder is covered by the approval (tree hash).
id = "aws-relational-database-compliance"
version = "1.0.0"
description = "aws-relational-database-compliance"
task_kinds = ["database-design"]
max_risk = "read_only"
evaluation_criteria = [
  "Provision, review or audit managed relational databases on AWS (Amazon RDS and Aurora PostgreSQL) with Terraform so they comply with enterprise CSS_1, CGS_1 and EAS_1 standards. Use when asked to create, scaffold, harden, audit or fix Terraform for a relational database, replica, snapshot restore or Serverless v2 cluster.",
]
+++

# AWS Relational Database Compliance

## 1. Purpose, Scope and Activation

Generate or review Terraform for **managed relational databases on AWS** (Amazon RDS, Aurora) so that it follows the local enterprise standards `CSS_1`, `CGS_1` and `EAS_1` (see the Traceability Matrix).

Activate when the user asks to create, scaffold, audit or fix a relational database, replica, restore or Serverless v2 cluster on AWS.

Out of scope: file and object storage (use `aws-file-storage-compliance`), DynamoDB and analytics stores (EAS_1 §3 gives selection hints only, no controls), application code. For out-of-scope or unsourced topics report `NOT VERIFIED`; never invent rules.

Terraform here is **implementation guidance** that realises the standards. The standards do not prescribe Terraform syntax, so every example must be reviewed and run through `terraform validate` and `terraform plan` before use.

## 2. Applicable Standards and Precedence

CSS (Cyber Security) > CGS (Cloud Governance) > EAS (Enterprise Architecture). Higher precedence wins on conflict. Never weaken a CSS or CGS requirement to simplify an implementation. No conflict between CSS_1, CGS_1 and EAS_1 was found in their current text.

## 3. Hard Guardrails (Non-negotiable)

MANDATORY means the source says "must", "must not", "prohibited", "blocked" or "Mandatory (Non-Negotiable)".

**Encryption (CSS_1)**
1. Managed relational instances use storage encryption at rest with **customer-managed keys (CMEK)**; provider-managed default keys are prohibited (§2).
2. Encryption is enabled at creation; never create unencrypted and encrypt later (§2).
3. `storage_encrypted` is `true` and `kms_key_id` binds to an approved customer-managed key ARN, never `null`, `default` or empty; omitting the attribute is prohibited (§4, §5).
4. Automated backups, point-in-time recovery logs and manual snapshots inherit the source key. Read replicas, standby instances, cross-region replicas, clones and restores are encrypted with approved customer-managed keys valid in their region (§2, §4, §5).
5. Only exemption: ephemeral containerized databases in isolated local CI runs (for example Testcontainers). Do not extend it (§3).

**Provider and topology (CGS_1)**
6. AWS is the sole approved platform; no non-AWS persistence provider resources and no multi-cloud persistence bridges (§1, §6).
7. Production uses managed RDS or Aurora, not database software on EC2 (§1).
8. Regions `us-east-1` and `us-east-2` only; non-US needs explicit Cloud Governance Board approval (§2).
9. Databases sit only in private database subnets without internet gateway routing; never in public VPC subnets (§2, §6).
10. Production enables Multi-AZ across independent Availability Zones (§2).
11. Production sets deletion protection to `true`; automated snapshot retention is 7 to 35 days (§5).
12. Every database resource carries tags `Environment` (`dev`/`stage`/`prod`), `CostCenter` (`CC-[0-9]{4}`), `Owner` (team email alias or service identifier), `DataClassification` (`Confidential`/`Internal`/`Public`) (§4).
13. Storage is `gp3` with storage autoscaling enabled; no `io1`/`io2` in dev or staging; non-production must not exceed `db.m6g.xlarge` without an approved capacity justification (§3, §6).
14. Development instances must support scheduled automated shutdown outside business hours (§5).

**Engine and architecture (EAS_1)**
15. Relational default is PostgreSQL v15+ / Aurora PostgreSQL. MySQL 8.x is legacy only, with no new MySQL databases. Oracle and SQL Server are blocked for new workloads (CTO exception waiver). SQLite and MS Access are prohibited in production (§2).
16. Database-per-service: no shared schemas or cross-database joins between services (§4, §5).
17. Schema changes use a migration framework (Flyway, Liquibase, ORM migrations); no manual DDL on non-ephemeral databases (§4). Terraform provisions infrastructure only; do not embed DDL.
18. Services access the database behind domain interfaces and do not leak persistence entities into API contracts; no dual writes across services without an outbox or event bus; no distributed relational 2PC (§4, §5).

**Recommendations (not mandatory; deviation is a warning with a stated reason)**
- R1 (CGS_1 §3, "should default"): variable or low-throughput workloads (fewer than 50 queries/sec baseline, or batch cycles) default to Aurora Serverless v2 capped at 16 ACUs.
- R2 (EAS_1 §3, "prefer"): document or JSON data prefers PostgreSQL `jsonb` unless scale or operational pattern warrants a dedicated store.
- R3 (EAS_1 §3): key-value high-throughput lookups use DynamoDB single-table design; analytical workloads over 100M rows export to the lakehouse instead of scaling OLTP.

**Agent behaviour (non-negotiable)**
19. **No unsupported claims.** Cite file and section for every rule applied or reported. Never invent IDs, requirements, limits, tags, SHAs or citations.
20. **Missing evidence is not compliance.** If a requirement cannot be confirmed from code or tool output, report `NOT VERIFIED`.
21. **Security-sensitive findings** (encryption, keys, network exposure, credentials) are Critical. Never downgrade them, suggest exceptions, or propose weakening a control. Never print real keys, account IDs or credentials; use placeholders.
22. **Unresolved conflicts** between standards, or between a user request and a MANDATORY rule, are stated explicitly and escalated to a human; never silently pick a side.
23. Never modify files under `standards/`. Use local files only; do not download or clone standards.

## 4. Required Inputs and Assumptions

If an input is unknown, leave a required variable with no default and mark the result `NOT VERIFIED`. Do not guess.

| Input | Notes |
|---|---|
| Environment | `dev`, `stage` or `prod` |
| Region | `us-east-1` or `us-east-2` |
| KMS key ARN | Existing customer-managed key in the same region (external prerequisite) |
| DB subnet IDs | Private subnets in at least two AZs with no internet gateway route (external prerequisite; verify) |
| Security group IDs | Existing groups restricting database ingress (external prerequisite) |
| Tag values | CostCenter, Owner, DataClassification |
| Engine and version | PostgreSQL-compatible, v15 or later |
| Instance class | Confirm it is offered for the engine and region |
| Throughput pattern | Decides whether R1 applies |

Assumptions: AWS is the only provider; Terraform 1.9 or later (variable validations reference other variables); placeholder values (engine minor version, storage sizes, minimum ACU, default retention) must be replaced from workload evidence.

## 5. Instructions

1. **Read the standards.** Read the three source files in the Traceability Matrix. `standards_kb.py` indexes only directories named `cyber-sec-standards`, `cloud-gov-standards` and `enterprise-arch-standards` found in an ancestor of its own folder. With the current `standards/` nesting it returned zero chunks when tested, so read the files directly. Files named `format.md` are templates, not requirements. CGS_1 §3 and the tag rules are scoped to database resources only.
2. **Gather inputs** (section 4). Do not guess key, subnet or tag values.
3. **Choose the pattern** and copy it from `assets/terraform/` (section 6). Adapt names; keep every validation block.
4. **Self-check** against section 7.
5. **Validate** using section 8 and `assets/verification-commands.md`.
6. **Report** using `assets/compliance-report-template.md`: each guardrail as `COMPLIANT (evidence)`, `VIOLATION` or `NOT VERIFIED` with file and section, plus assumptions and unresolved conflicts. State overall compliance only when every applicable mandatory rule is evidenced.

## 6. Terraform Assets

| Asset | Type | Use |
|---|---|---|
| [versions.tf](assets/terraform/versions.tf), [variables.tf](assets/terraform/variables.tf), [main.tf](assets/terraform/main.tf), [outputs.tf](assets/terraform/outputs.tf) | **Complete root configuration** (default Aurora PostgreSQL) | Copy together into one directory. External prerequisites: KMS key, private subnets, security groups. |
| [alt-rds-instance.tf.example](assets/terraform/alt-rds-instance.tf.example) | Partial snippet, alternative | Single RDS PostgreSQL instance with `gp3` and autoscaling; replaces the Aurora cluster in `main.tf`. |
| [alt-aurora-serverless-v2.tf.example](assets/terraform/alt-aurora-serverless-v2.tf.example) | Partial snippet, alternative (R1) | Serverless v2 with the 16 ACU cap. |
| [replica-rds.tf.example](assets/terraform/replica-rds.tf.example) | Partial snippet | Encrypted same-region replica for the RDS instance. Aurora readers inherit the cluster key. |
| [restore-aurora-snapshot.tf.example](assets/terraform/restore-aurora-snapshot.tf.example) | Partial snippet | Restore with an explicit customer-managed key. |

Core pattern (from `main.tf` and `variables.tf`):

```hcl
resource "aws_rds_cluster" "this" {
  engine            = "aurora-postgresql"
  storage_encrypted = true               # CSS_1 s4
  kms_key_id        = var.kms_key_arn    # customer-managed key ARN, validated in variables.tf
  deletion_protection = local.is_prod    # CGS_1 s5
  tags              = local.tags         # CGS_1 s4: Environment, CostCenter, Owner, DataClassification
  # remaining arguments: see assets/terraform/main.tf
}
```

Notes:
- Aurora storage is service-managed. Do not add `allocated_storage` or `storage_type` to an Aurora cluster; whether CGS_1 §3 `gp3` and autoscaling apply to Aurora is not stated (`NOT VERIFIED`).
- The Aurora Multi-AZ mapping (a second instance placed in another AZ) and the two-subnet minimum are implementation guidance, not standard text. Confirm placement with the post-deployment commands before claiming Multi-AZ.
- `instance_class` has no default. `db.m6g.*` may not be offered for Aurora PostgreSQL; if a non-production class outside the cap is needed, a capacity justification reference is required (`capacity_justification_ref`). Terraform only checks that it is non-empty; the approval is `NOT VERIFIED`.
- `manage_master_user_password` keeps credentials in AWS Secrets Manager. It is a design choice, not a standards requirement.
- Dev scheduled shutdown (guardrail 14): CGS_1 §5 specifies no mechanism, so no Terraform is provided. Implement with an AWS-native scheduler chosen by the team and report `NOT VERIFIED` until a reviewer confirms it.
- Controlling access with IAM roles and policies, database authentication method, TLS and logging have no local standard; report them `NOT VERIFIED`.

## 7. Anti-pattern Self-check

Confirm none appear before returning code (each is prohibited in the cited section):
- Encryption disabled or omitted; blank, null or `default` key; unencrypted replica, clone or restore (CSS_1 §5).
- Non-AWS persistence provider resources, for example `google_sql_*` or `azurerm_*` (CGS_1 §6).
- Public subnets, publicly accessible instances, internet-routed database subnets (CGS_1 §6).
- `io1`/`io2` in dev or staging (CGS_1 §6).
- Oracle, SQL Server, new MySQL, SQLite in production (EAS_1 §2).
- Shared schemas between services, embedded or manual DDL, dual writes, 2PC (EAS_1 §4, §5).
- Plaintext passwords or secrets in `.tf` files, defaults or outputs. No local standard covers secrets; report as unsourced hygiene, not as a standards violation.

## 8. Validation Commands and Expected Checks

```bash
terraform fmt -check -recursive && terraform init -backend=false && terraform validate
grep -RInE 'google_sql|azurerm_' --include='*.tf' .
python3 .agents/skills/skill-maker/scripts/validator.py --skill-path <path-to-SKILL.md>
```

The full command set with expected results, post-deployment AWS CLI evidence commands, and a failure and recovery table are in [verification-commands.md](assets/verification-commands.md). Report any command you did not run as `not executed`.

What `validator.py` actually checks (from its source): frontmatter present with `name` and `description`; H2 headings containing guardrail/rule/requirement, instruction/step/workflow, and traceability; non-AWS provider declarations in Terraform syntax; the words "encrypted", "kms" and "iam" when "database", "postgres", "rds" or "aurora" appear (substring match); the literal text for a public-access flag set to true; a table cell equal to `CSS`, `CGS` or `EAS`; and any 7-40 character hex string. It reads only `SKILL.md`, not `assets/`. It does **not** verify SHAs against a repository, that matrix files exist, regions, subnets, Multi-AZ, tags, sizing, engines, Terraform correctness or the accuracy of citations. A pass or a clean `terraform validate` does not prove compliance. Its IAM warning attributes a rule to CSS_1, but CSS_1 has no IAM database authentication requirement.

## 9. Troubleshooting

See the failure and recovery table in [verification-commands.md](assets/verification-commands.md). Rules for every failure: fix the cause, never relax a validation, never fall back to a default key, never suggest an exception.

## 10. Traceability Matrix

Git commit SHA: Not available - local source is not verified against a Git commit (the workspace is not a git repository). For change detection only, SHA-256 content-hash prefixes at authoring time: CSS_1 `0b84a89ec385`, CGS_1 `26e35cebab16`, EAS_1 `8e698d5a73da`. These are not commit SHAs; recompute when files change.

| Precedence | Standard | Local source file | Rule or section | Requirement |
|---|---|---|---|---|
| 1 | CSS | `standards/cyber-sec-standards/CSS_1-Relational_Database_Encryption_at_Rest.md` | §2 Normative Guardrails | CMEK at-rest encryption at creation, inherited by backups, snapshots, replicas |
| 1 | CSS | `standards/cyber-sec-standards/CSS_1-Relational_Database_Encryption_at_Rest.md` | §3 Boundary & Scope | Only exemption: ephemeral local CI databases |
| 1 | CSS | `standards/cyber-sec-standards/CSS_1-Relational_Database_Encryption_at_Rest.md` | §4 Declarative Enforcement, §5 Anti-Patterns | `storage_encrypted` true, non-empty customer-managed `kms_key_id`; no unencrypted replicas, clones, restores |
| 2 | CGS | `standards/cloud-gov-standards/CGS_1-approved_cloud_provider_for_managed_databases.md` | §1 Baseline | AWS only; RDS or Aurora, not EC2-hosted databases |
| 2 | CGS | `standards/cloud-gov-standards/CGS_1-approved_cloud_provider_for_managed_databases.md` | §2 Resource & Topology | `us-east-1`/`us-east-2`, private subnets, production Multi-AZ |
| 2 | CGS | `standards/cloud-gov-standards/CGS_1-approved_cloud_provider_for_managed_databases.md` | §3 FinOps & Sizing | Serverless v2 recommendation (16 ACU cap); non-production cap `db.m6g.xlarge`; `gp3` with autoscaling |
| 2 | CGS | `standards/cloud-gov-standards/CGS_1-approved_cloud_provider_for_managed_databases.md` | §4 Mandatory Metadata | Four required tags and value formats |
| 2 | CGS | `standards/cloud-gov-standards/CGS_1-approved_cloud_provider_for_managed_databases.md` | §5 Drift & Lifecycle | Production deletion protection; 7-35 day retention; dev auto-stop support |
| 2 | CGS | `standards/cloud-gov-standards/CGS_1-approved_cloud_provider_for_managed_databases.md` | §6 Prohibited Topologies | No public subnets, no `io1`/`io2` in dev/staging, no non-AWS persistence resources |
| 3 | EAS | `standards/enterprise-arch-standards/EAS_1-approved_enterprise_database_engines.md` | §2 Technology Classification | PostgreSQL 15+ strategic; MySQL legacy; Oracle, SQL Server, SQLite prohibited |
| 3 | EAS | `standards/enterprise-arch-standards/EAS_1-approved_enterprise_database_engines.md` | §3 Decision Matrix | Selection hints (recommendations R2, R3) |
| 3 | EAS | `standards/enterprise-arch-standards/EAS_1-approved_enterprise_database_engines.md` | §4 Boundaries, §5 Anti-Patterns | Database-per-service, migration framework, data access layer, no dual writes or 2PC |

## 11. Known Limitations, Conflicts and NOT VERIFIED Items

- No conflicts found between CSS_1, CGS_1 and EAS_1.
- `NOT VERIFIED`: IAM database authentication, TLS, secrets management, logging and monitoring (no local standard); dev scheduled shutdown mechanism; environment-to-retention mapping; Aurora applicability of `gp3`/autoscaling and the `db.m6g.xlarge` cap; availability of `db.m6g.*` for Aurora; capacity-justification approval; Multi-AZ placement and private subnet routing until checked against the deployed environment.
- Terraform was not installed in the authoring environment, so the `.tf` files were not run through `terraform fmt`, `validate` or `plan`. Only structural checks (balanced braces, forbidden patterns, paths) were run.
- No Rosetta-specific format documentation or example skill exists in this workspace. This skill follows `reference/reference-skill-maker.SKILL_V2.md`; confirm against Rosetta's real requirements before publishing.
