locals {
  # Every resource name ends with the stage, e.g. "-dev" / "-prod".
  suffix      = "-${var.stage}"
  api_name    = "${var.project}${local.suffix}"
  lambda_name = "${var.project}-api${local.suffix}"
  role_name   = "${var.project}-api-role${local.suffix}"
}

# Placeholder code package. The real Rust binary is pushed by
# `cd backend && make deploy <stage>`, so terraform ignores code changes below.
data "archive_file" "placeholder" {
  type             = "zip"
  source_file      = "${path.module}/placeholder/bootstrap"
  output_file_mode = "0755"
  output_path      = "${path.module}/placeholder/bootstrap.zip"
}

data "aws_iam_policy_document" "lambda_assume" {
  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "lambda" {
  name               = local.role_name
  assume_role_policy = data.aws_iam_policy_document.lambda_assume.json
}

resource "aws_iam_role_policy_attachment" "lambda_basic" {
  role       = aws_iam_role.lambda.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

resource "aws_cloudwatch_log_group" "lambda" {
  name              = "/aws/lambda/${local.lambda_name}"
  retention_in_days = var.log_retention_days
}

resource "aws_lambda_function" "api" {
  function_name = local.lambda_name
  role          = aws_iam_role.lambda.arn
  runtime       = "provided.al2023"
  handler       = "bootstrap"
  architectures = [var.lambda_architecture]
  memory_size   = var.lambda_memory_mb
  timeout       = var.lambda_timeout_s

  filename         = data.archive_file.placeholder.output_path
  source_code_hash = data.archive_file.placeholder.output_base64sha256

  environment {
    variables = {
      STAGE          = var.stage
      RUST_LOG       = "info"
      RUST_BACKTRACE = "1"
      JWT_SECRET_ID  = aws_secretsmanager_secret.jwt.name
      # DSQL takes an IAM token, not a password, so the endpoint is the whole
      # connection config the lambda needs.
      DATABASE_ENDPOINT = local.db_endpoint
      # Where the messenger's Management API client pushes messages back out
      # through -- see messenger.tf.
      MESSENGER_MANAGEMENT_ENDPOINT = local.messenger_management_endpoint
      # Bucket core::media presigns uploads into and reads/writes/deletes
      # from directly -- see media.tf.
      MEDIA_BUCKET = aws_s3_bucket.media.bucket
    }
  }

  depends_on = [
    aws_iam_role_policy_attachment.lambda_basic,
    aws_cloudwatch_log_group.lambda,
    aws_iam_role_policy.api_read_jwt_secret,
    aws_iam_role_policy.api_chime,
    aws_iam_role_policy.api_dsql,
    aws_iam_role_policy.api_messenger_management,
    aws_iam_role_policy.api_media,
  ]

  # Code is owned by the backend deploy, not terraform.
  lifecycle {
    ignore_changes = [filename, source_code_hash]
  }
}

resource "aws_apigatewayv2_api" "api" {
  name          = local.api_name
  protocol_type = "HTTP"
}

resource "aws_apigatewayv2_integration" "lambda" {
  api_id                 = aws_apigatewayv2_api.api.id
  integration_type       = "AWS_PROXY"
  integration_uri        = aws_lambda_function.api.invoke_arn
  payload_format_version = "2.0"
}

resource "aws_apigatewayv2_route" "default" {
  api_id    = aws_apigatewayv2_api.api.id
  route_key = "$default"
  target    = "integrations/${aws_apigatewayv2_integration.lambda.id}"
}

resource "aws_cloudwatch_log_group" "api" {
  name              = "/aws/apigateway/${local.api_name}"
  retention_in_days = var.log_retention_days
}

resource "aws_apigatewayv2_stage" "default" {
  api_id      = aws_apigatewayv2_api.api.id
  name        = "$default"
  auto_deploy = true

  access_log_settings {
    destination_arn = aws_cloudwatch_log_group.api.arn
    format = jsonencode({
      requestId  = "$context.requestId"
      httpMethod = "$context.httpMethod"
      path       = "$context.path"
      status     = "$context.status"
      latency    = "$context.responseLatency"
    })
  }
}

resource "aws_lambda_permission" "api_gateway" {
  statement_id  = "AllowExecutionFromAPIGateway${local.suffix}"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.api.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_apigatewayv2_api.api.execution_arn}/*/*"
}
