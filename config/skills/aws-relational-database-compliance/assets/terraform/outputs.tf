output "cluster_endpoint" {
  description = "Writer endpoint."
  value       = aws_rds_cluster.this.endpoint
}

output "master_user_secret_arn" {
  description = "Secrets Manager ARN holding the generated master credential (an ARN, not the secret)."
  value       = aws_rds_cluster.this.master_user_secret[0].secret_arn
}
