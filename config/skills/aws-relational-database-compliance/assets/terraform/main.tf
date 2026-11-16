# Aurora PostgreSQL cluster with customer-managed key encryption in private subnets.
# Standards: CSS_1 s2, s4; CGS_1 s2, s4, s5; EAS_1 s2.
# Prerequisites supplied as variables (not created here): KMS key, private subnets, security groups.

resource "aws_db_subnet_group" "this" {
  name       = "${var.environment}-db-subnets"
  subnet_ids = var.db_subnet_ids
  tags       = local.tags
}

resource "aws_rds_cluster" "this" {
  cluster_identifier = "${var.environment}-app-db"
  engine             = "aurora-postgresql"
  engine_version     = var.engine_version

  # CSS_1 s4: encryption on, bound to a customer-managed key, set at creation.
  storage_encrypted = true
  kms_key_id        = var.kms_key_arn

  db_subnet_group_name   = aws_db_subnet_group.this.name
  vpc_security_group_ids = var.vpc_security_group_ids

  master_username             = var.master_username
  manage_master_user_password = true

  backup_retention_period   = var.backup_retention_days
  copy_tags_to_snapshot     = true
  deletion_protection       = local.is_prod
  skip_final_snapshot       = !local.is_prod
  final_snapshot_identifier = local.is_prod ? "${var.environment}-app-db-final" : null

  tags = local.tags
}

resource "aws_rds_cluster_instance" "this" {
  # Production: two instances so the cluster can span Availability Zones (implementation guidance for CGS_1 s2).
  count = local.is_prod ? 2 : 1

  identifier         = "${var.environment}-app-db-${count.index}"
  cluster_identifier = aws_rds_cluster.this.id
  engine             = aws_rds_cluster.this.engine
  engine_version     = aws_rds_cluster.this.engine_version
  instance_class     = var.instance_class

  publicly_accessible = false
  tags                = local.tags
}
