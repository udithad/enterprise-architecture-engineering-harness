# Relational database verification commands and failure recovery

Commands marked "needs credentials" were not executed when this skill was authored. Report them as `not executed` unless you ran them and have the output.

## Pre-deployment (local)

```bash
cd <directory containing the .tf files>
terraform fmt -check -recursive        # skip with explicit note if terraform is not installed
terraform init -backend=false
terraform validate
terraform plan -out=tfplan             # needs credentials and real inputs
terraform show -json tfplan > plan.json  # inspect storage_encrypted, kms_key_id, publicly_accessible, tags, deletion_protection

# Prohibited non-AWS database resources (CGS_1 s6)
grep -RInE 'google_sql|azurerm_' --include='*.tf' .

# Obvious plaintext secrets (unsourced hygiene check; heuristic only)
grep -RInE '(password|secret)[[:space:]]*=[[:space:]]*"[^"]+"' --include='*.tf' .
```

Expected: `validate` reports success; the plan shows `storage_encrypted = true`, a non-empty `kms_key_id` ARN, `publicly_accessible = false`, and all four tags on every database resource. A clean `validate` does not prove compliance.

## Post-deployment evidence (needs credentials)

```bash
aws rds describe-db-clusters --db-cluster-identifier <id> \
  --query 'DBClusters[].{enc:StorageEncrypted,key:KmsKeyId,del:DeletionProtection,az:AvailabilityZones,ret:BackupRetentionPeriod}'
aws rds describe-db-instances \
  --query 'DBInstances[].{id:DBInstanceIdentifier,az:AvailabilityZone,pub:PubliclyAccessible,class:DBInstanceClass}'
aws rds describe-db-engine-versions --engine aurora-postgresql --query 'DBEngineVersions[].EngineVersion'
aws rds describe-orderable-db-instance-options --engine aurora-postgresql --engine-version <ver> \
  --query 'OrderableDBInstanceOptions[].DBInstanceClass'
aws ec2 describe-route-tables --filters Name=association.subnet-id,Values=<subnet-id>   # no route to an igw- target
aws rds describe-db-snapshots --db-instance-identifier <id> --query 'DBSnapshots[].{id:DBSnapshotIdentifier,enc:Encrypted,key:KmsKeyId}'
```

Expected: encrypted true with the approved key; deletion protection true in production; retention between 7 and 35; instances in more than one Availability Zone in production; no internet-gateway route on database subnets.

## Common failures

| Symptom | Likely cause and recovery |
|---|---|
| `kms_key_arn` validation error | Key ID, alias, empty value, or a key in another region. Supply the full customer-managed key ARN in the deployment region. Do not relax the validation. |
| Validation cannot reference another variable | Terraform older than 1.9. Upgrade, or move the check into a `lifecycle { precondition { } }` block. |
| `instance_class` validation error in non-production | Class is outside the convenience allow-list. Use an allowed class, or set `capacity_justification_ref` to a real approval reference. Do not edit the cap. |
| Instance class or engine version unavailable | Use the orderable-options and engine-version commands above. Never silently pick a larger non-production class; record `NOT VERIFIED` or request a justification. |
| Production apply asks for a final snapshot identifier | `skip_final_snapshot` is false in production. Keep `final_snapshot_identifier` set. |
| KMS access denied on create | The key policy must allow the RDS service and the deploying role. Fix the key policy; never fall back to a default key. |
| Subnet group rejected | RDS needs subnets in at least two Availability Zones. Supply private subnets from independent AZs. |
| Replica, clone or restore ends up unencrypted | Missing explicit `kms_key_id`. Set it and recreate; never encrypt retroactively (CSS_1 s2). |
