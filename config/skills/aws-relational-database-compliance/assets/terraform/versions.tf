# Part of the complete root configuration (versions.tf, variables.tf, main.tf, outputs.tf).
# Standards: CGS_1 section 1 (AWS only), section 2 (approved regions).

terraform {
  # 1.9 or later: variable validations below reference other variables.
  required_version = ">= 1.9"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
}

provider "aws" {
  region = var.aws_region
}
