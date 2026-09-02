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

output "backend_env_file" {
  description = "Paste this into backend/.env.<stage>."
  value       = <<-EOT
    LAMBDA_FUNCTION_NAME=${aws_lambda_function.api.function_name}
    API_URL=${aws_apigatewayv2_stage.default.invoke_url}
  EOT
}
