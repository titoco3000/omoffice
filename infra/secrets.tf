locals {
  jwt_secret_name = "${var.project}/jwt-key${local.suffix}"
  rotator_name    = "${var.project}-jwt-rotator${local.suffix}"
}

# ---------------------------------------------------------------------------
# The signing key
# ---------------------------------------------------------------------------

resource "aws_secretsmanager_secret" "jwt" {
  name        = local.jwt_secret_name
  description = "HMAC key the ${var.stage} API signs JWTs with. Rotated weekly."

  # Sample project: let `make destroy` actually free the name instead of
  # holding it for the default 30-day recovery window.
  recovery_window_in_days = 0
}

# Seed value so the API works before the first rotation ever runs.
resource "random_password" "jwt_initial" {
  length  = 64
  special = false
}

resource "aws_secretsmanager_secret_version" "initial" {
  secret_id     = aws_secretsmanager_secret.jwt.id
  secret_string = random_password.jwt_initial.result

  # After the first rotation the current version is the rotator's, not this one.
  lifecycle {
    ignore_changes = [secret_string, version_stages]
  }
}

resource "aws_secretsmanager_secret_rotation" "jwt" {
  secret_id           = aws_secretsmanager_secret.jwt.id
  rotation_lambda_arn = aws_lambda_function.rotator.arn

  # Don't burn a rotation on every apply; the schedule is the point.
  rotate_immediately = false

  rotation_rules {
    schedule_expression = var.jwt_rotation_schedule
  }

  depends_on = [
    aws_lambda_permission.rotator_secretsmanager,
    aws_secretsmanager_secret_version.initial,
  ]
}

# ---------------------------------------------------------------------------
# Rotation lambda (owned end to end by terraform, unlike the API lambda)
# ---------------------------------------------------------------------------

data "archive_file" "rotator" {
  type        = "zip"
  source_dir  = "${path.module}/rotator"
  output_path = "${path.module}/rotator.zip"
}

resource "aws_iam_role" "rotator" {
  name               = "${local.rotator_name}-role"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume.json
}

resource "aws_iam_role_policy_attachment" "rotator_basic" {
  role       = aws_iam_role.rotator.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

data "aws_iam_policy_document" "rotator" {
  statement {
    sid = "RotateTheJwtSecret"
    actions = [
      "secretsmanager:DescribeSecret",
      "secretsmanager:GetSecretValue",
      "secretsmanager:PutSecretValue",
      "secretsmanager:UpdateSecretVersionStage",
    ]
    resources = [aws_secretsmanager_secret.jwt.arn]
  }

  statement {
    sid = "GenerateKeyMaterial"
    # GetRandomPassword is account-level; it takes no resource.
    actions   = ["secretsmanager:GetRandomPassword"]
    resources = ["*"]
  }
}

resource "aws_iam_role_policy" "rotator" {
  name   = "${local.rotator_name}-policy"
  role   = aws_iam_role.rotator.id
  policy = data.aws_iam_policy_document.rotator.json
}

resource "aws_cloudwatch_log_group" "rotator" {
  name              = "/aws/lambda/${local.rotator_name}"
  retention_in_days = var.log_retention_days
}

resource "aws_lambda_function" "rotator" {
  function_name = local.rotator_name
  role          = aws_iam_role.rotator.arn
  runtime       = "python3.12"
  handler       = "index.lambda_handler"
  timeout       = 30

  filename         = data.archive_file.rotator.output_path
  source_code_hash = data.archive_file.rotator.output_base64sha256

  depends_on = [
    aws_iam_role_policy_attachment.rotator_basic,
    aws_cloudwatch_log_group.rotator,
  ]
}

resource "aws_lambda_permission" "rotator_secretsmanager" {
  statement_id  = "AllowSecretsManagerRotation${local.suffix}"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.rotator.function_name
  principal     = "secretsmanager.amazonaws.com"
  source_arn    = aws_secretsmanager_secret.jwt.arn
}

# ---------------------------------------------------------------------------
# Read access for the API lambda
# ---------------------------------------------------------------------------

data "aws_iam_policy_document" "api_read_jwt_secret" {
  statement {
    sid = "ReadTheJwtSecret"
    # Both the current key and the one it replaced, so tokens issued just
    # before a rotation stay valid until they expire.
    actions   = ["secretsmanager:GetSecretValue"]
    resources = [aws_secretsmanager_secret.jwt.arn]
  }
}

resource "aws_iam_role_policy" "api_read_jwt_secret" {
  name   = "${local.lambda_name}-jwt-secret"
  role   = aws_iam_role.lambda.id
  policy = data.aws_iam_policy_document.api_read_jwt_secret.json
}
