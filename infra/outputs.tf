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

output "database_cluster_id" {
  description = "Aurora DSQL cluster backing this stage."
  value       = aws_dsql_cluster.main.identifier
}

output "database_endpoint" {
  description = "Host the API and the SQL shell connect to (IAM token as the password)."
  value       = local.db_endpoint
}

output "messenger_ws_url" {
  description = "Browser-facing wss:// URL for the messenger's WebSocket API (?token=...)."
  value       = local.messenger_ws_url
}

output "messenger_management_endpoint" {
  description = "https:// endpoint the lambda posts messenger messages back through."
  value       = local.messenger_management_endpoint
}

output "media_bucket" {
  description = "S3 bucket the API lambda presigns avatar/icon uploads into."
  value       = aws_s3_bucket.media.bucket
}

output "media_url" {
  description = "Public URL a stored `picture` key resolves under."
  value       = "https://${aws_cloudfront_distribution.media.domain_name}"
}

output "backend_env_file" {
  description = "Paste this into backend/.env.<stage>."
  value       = <<-EOT
    LAMBDA_FUNCTION_NAME=${aws_lambda_function.api.function_name}
    API_URL=${aws_apigatewayv2_stage.default.invoke_url}
    JWT_SECRET_ID=${aws_secretsmanager_secret.jwt.name}
    DATABASE_ENDPOINT=${local.db_endpoint}
    MESSENGER_MANAGEMENT_ENDPOINT=${local.messenger_management_endpoint}
    MEDIA_BUCKET=${aws_s3_bucket.media.bucket}
    CHIME_WEBHOOK_SECRET=${nonsensitive(random_password.chime_webhook_secret.result)}
    MAP_EVENTS_ENDPOINT=${local.map_events_endpoint}
    MAP_AUTHORIZER_FUNCTION_NAME=${aws_lambda_function.map_authorizer.function_name}
  EOT
}

output "map_events_endpoint" {
  description = "AppSync Events HTTP host the office map's live moves go through."
  value       = local.map_events_endpoint
}

output "map_authorizer_function_name" {
  description = "Lambda the backend deploy pushes the map-authorizer binary to."
  value       = aws_lambda_function.map_authorizer.function_name
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
    MESSENGER_WS_URL=${local.messenger_ws_url}
    MEDIA_URL=https://${aws_cloudfront_distribution.media.domain_name}
  EOT
}
