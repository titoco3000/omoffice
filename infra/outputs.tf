output "api_url" {
  description = "Public HTTP API base URL."
  value       = aws_apigatewayv2_stage.default.invoke_url
}

output "lambda_function_name" {
  description = "Lambda the backend deploy pushes code to."
  value       = aws_lambda_function.api.function_name
}

output "lambda_role_arn" {
  description = "Lambda execution role."
  value       = aws_iam_role.lambda.arn
}

output "lambda_log_group" {
  description = "CloudWatch log group for the lambda."
  value       = aws_cloudwatch_log_group.lambda.name
}

output "region" {
  description = "AWS region the stack lives in."
  value       = var.region
}

output "jwt_secret_name" {
  description = "Secrets Manager secret holding the JWT signing key."
  value       = aws_secretsmanager_secret.jwt.name
}

output "jwt_rotation_schedule" {
  description = "When that key rotates (UTC)."
  value       = var.jwt_rotation_schedule
}

output "backend_env_file" {
  description = "Paste this into backend/.env.<stage>."
  value       = <<-EOT
    LAMBDA_FUNCTION_NAME=${aws_lambda_function.api.function_name}
    API_URL=${aws_apigatewayv2_stage.default.invoke_url}
    JWT_SECRET_ID=${aws_secretsmanager_secret.jwt.name}
  EOT
}

output "frontend_bucket" {
  description = "S3 bucket the frontend deploy syncs the built assets into."
  value       = aws_s3_bucket.frontend.bucket
}

output "frontend_distribution_id" {
  description = "CloudFront distribution the frontend deploy invalidates."
  value       = aws_cloudfront_distribution.frontend.id
}

output "frontend_url" {
  description = "Public URL of the frontend."
  value       = "https://${aws_cloudfront_distribution.frontend.domain_name}"
}

output "frontend_env_file" {
  description = "Paste this into frontend/.env.<stage>."
  value       = <<-EOT
    API_URL=${aws_apigatewayv2_stage.default.invoke_url}
    FRONTEND_BUCKET=${aws_s3_bucket.frontend.bucket}
    FRONTEND_DISTRIBUTION_ID=${aws_cloudfront_distribution.frontend.id}
    FRONTEND_URL=https://${aws_cloudfront_distribution.frontend.domain_name}
  EOT
}
