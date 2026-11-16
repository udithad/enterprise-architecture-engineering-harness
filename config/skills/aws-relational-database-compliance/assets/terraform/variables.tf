# Inputs and guardrail validations for the relational database root configuration.
# Every validation cites the local standard it encodes. Values marked "placeholder" are not standard values.

variable "aws_region" {
  description = "Deployment region."
  type        = string
  default     = "us-east-1"

  validation {
    condition     = contains(["us-east-1", "us-east-2"], var.aws_region)
    error_message = "Only us-east-1 and us-east-2 are approved (CGS_1 s2). Other regions need Cloud Governance Board approval."
  }
}

variable "environment" {
  description = "Environment tag value and lifecycle switch."
  type        = string

  validation {
    condition     = contains(["dev", "stage", "prod"], var.environment)
    error_message = "Environment must be dev, stage or prod (CGS_1 s4)."
  }
}

variable "cost_center" {
  description = "Corporate billing code."
  type        = string

  validation {
    condition     = can(regex("^CC-[0-9]{4}$", var.cost_center))
    error_message = "CostCenter must match CC-[0-9]{4} (CGS_1 s4)."
  }
}

variable "owner" {
  description = "Team email alias or service identifier."
  type        = string

  validation {
    condition     = length(trimspace(var.owner)) > 0
    error_message = "Owner is required (CGS_1 s4)."
  }
}

variable "data_classification" {
  description = "Data classification tag value."
  type        = string

  validation {
    condition     = contains(["Confidential", "Internal", "Public"], var.data_classification)
    error_message = "DataClassification must be Confidential, Internal or Public (CGS_1 s4)."
  }
}

variable "kms_key_arn" {
  description = "Customer-managed KMS key ARN in the deployment region (external prerequisite). No default on purpose."
  type        = string

  validation {
    condition = (
      can(regex("^arn:aws[a-z-]*:kms:[a-z0-9-]+:[0-9]{12}:key/.+$", var.kms_key_arn)) &&
      try(split(":", var.kms_key_arn)[3], "") == var.aws_region
    )
    error_message = "kms_key_arn must be a full customer-managed key ARN in the deployment region; null, 'default' and empty values are prohibited (CSS_1 s2, s4)."
  }
}

variable "db_subnet_ids" {
  description = "Private database subnet IDs without an internet gateway route (external prerequisite; routing must be verified separately)."
  type        = list(string)

  validation {
    condition     = length(var.db_subnet_ids) >= 2
    error_message = "Provide private DB subnets in at least two Availability Zones (implementation guidance for CGS_1 s2 Multi-AZ)."
  }
}

variable "vpc_security_group_ids" {
  description = "Existing security groups that restrict database ingress (external prerequisite)."
  type        = list(string)
}

variable "backup_retention_days" {
  description = "Placeholder default. CGS_1 s5 allows 7-35 days depending on environment classification but gives no mapping."
  type        = number
  default     = 7

  validation {
    condition     = var.backup_retention_days >= 7 && var.backup_retention_days <= 35
    error_message = "Automated snapshot retention must be 7-35 days (CGS_1 s5)."
  }
}

variable "engine_version" {
  description = "PostgreSQL-compatible major version 15 or later. Placeholder default: confirm availability with aws rds describe-db-engine-versions."
  type        = string
  default     = "15.4"

  validation {
    condition     = can(regex("^(1[5-9]|[2-9][0-9])(\\.|$)", var.engine_version))
    error_message = "EAS_1 s2 requires PostgreSQL v15 or later."
  }
}

variable "instance_class" {
  description = "DB instance class. No default: confirm the class is offered for the engine and region (db.m6g.* availability for Aurora is NOT VERIFIED)."
  type        = string

  validation {
    condition = (
      var.environment == "prod" ||
      contains(["db.m6g.medium", "db.m6g.large", "db.m6g.xlarge"], var.instance_class) ||
      length(trimspace(var.capacity_justification_ref)) > 0
    )
    error_message = "Non-production must not exceed db.m6g.xlarge without an approved capacity justification (CGS_1 s3). The allow-list is a convenience subset; set capacity_justification_ref to the approval reference for any other class."
  }
}

variable "capacity_justification_ref" {
  description = "Reference to an approved capacity justification (CGS_1 s3). Terraform only checks it is non-empty; the approval itself is NOT VERIFIED here."
  type        = string
  default     = ""
}

variable "master_username" {
  description = "Master user name. The password is generated and stored in AWS Secrets Manager (design choice, not a standard)."
  type        = string
  default     = "dbadmin"
}

locals {
  is_prod = var.environment == "prod"

  tags = {
    Environment        = var.environment
    CostCenter         = var.cost_center
    Owner              = var.owner
    DataClassification = var.data_classification
  }
}
