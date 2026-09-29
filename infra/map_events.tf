# ---------------------------------------------------------------------------
# The office map's live moves, off the API lambda's hot path: browsers
# publish their own moves straight to this AppSync Events API and subscribe
# to everyone else's in their org. The API lambda keeps persistence and
# hands out the tokens this API accepts (`GET /map/org/{org_id}/realtime`);
# see `backend/lib/features/map/realtime.rs`.
#
# Every connect/subscribe/publish is authorized by a small lambda of its
# own, the backend crate's `map-authorizer` binary -- a JWT check, no
# database. Like the API lambda, terraform creates it with placeholder code
# and `cd backend && make deploy <stage>` pushes the real binary.
# ---------------------------------------------------------------------------

locals {
  map_authorizer_name = "${var.project}-map-authorizer${local.suffix}"
}

resource "aws_iam_role" "map_authorizer" {
  name               = "${local.map_authorizer_name}-role"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume.json
}

resource "aws_iam_role_policy_attachment" "map_authorizer_basic" {
  role       = aws_iam_role.map_authorizer.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

# Same signing key as the API lambda, from the same place -- it verifies
# tokens that one signs.
resource "aws_iam_role_policy" "map_authorizer_read_jwt_secret" {
  name   = "${local.map_authorizer_name}-jwt-secret"
  role   = aws_iam_role.map_authorizer.id
  policy = data.aws_iam_policy_document.api_read_jwt_secret.json
}

resource "aws_cloudwatch_log_group" "map_authorizer" {
  name              = "/aws/lambda/${local.map_authorizer_name}"
  retention_in_days = var.log_retention_days
}

resource "aws_lambda_function" "map_authorizer" {
  function_name = local.map_authorizer_name
  role          = aws_iam_role.map_authorizer.arn
  runtime       = "provided.al2023"
  handler       = "bootstrap"
  architectures = [var.lambda_architecture]
  memory_size   = 128
  timeout       = 5

  filename         = data.archive_file.placeholder.output_path
  source_code_hash = data.archive_file.placeholder.output_base64sha256

  environment {
    variables = {
      RUST_LOG      = "info"
      JWT_SECRET_ID = aws_secretsmanager_secret.jwt.name
    }
  }

  depends_on = [
    aws_iam_role_policy_attachment.map_authorizer_basic,
    aws_iam_role_policy.map_authorizer_read_jwt_secret,
    aws_cloudwatch_log_group.map_authorizer,
  ]

  # Code is owned by the backend deploy, not terraform.
  lifecycle {
    ignore_changes = [filename, source_code_hash]
  }
}

resource "aws_appsync_api" "map_events" {
  name = "${local.api_name}-map-events"

  event_config {
    auth_provider {
      auth_type = "AWS_LAMBDA"
      lambda_authorizer_config {
        authorizer_uri = aws_lambda_function.map_authorizer.arn
        # The authorizer answers with its own `ttlOverride` (the token's
        # remaining lifetime), which takes precedence over this.
        authorizer_result_ttl_in_seconds = 300
      }
    }

    connection_auth_mode {
      auth_type = "AWS_LAMBDA"
    }

    default_publish_auth_mode {
      auth_type = "AWS_LAMBDA"
    }

    default_subscribe_auth_mode {
      auth_type = "AWS_LAMBDA"
    }
  }
}

resource "aws_lambda_permission" "map_authorizer_appsync" {
  statement_id  = "AllowExecutionFromAppSync${local.suffix}"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.map_authorizer.function_name
  principal     = "appsync.amazonaws.com"
  source_arn    = aws_appsync_api.map_events.api_arn
}

# `/map/{org_id}/{user_id}`. A subscriber's data frame carries no channel,
# so `onPublish` stamps the sender -- the channel's last segment, which the
# authorizer has already pinned to the publisher's own id -- onto each
# event, and passes only `x`/`y`/`facing` through from what the client sent.
resource "aws_appsync_channel_namespace" "map" {
  name   = "map"
  api_id = aws_appsync_api.map_events.api_id

  code_handlers = <<-EOT
    export function onPublish(ctx) {
      const segments = ctx.info.channel.path.split('/');
      const userId = segments[segments.length - 1];
      return ctx.events.map((event) => ({
        id: event.id,
        payload: { user_id: userId, x: event.payload.x, y: event.payload.y, facing: event.payload.facing },
      }));
    }
  EOT
}

locals {
  # The HTTP host: the backend hands it to the browser, which names it as
  # `host` in its auth header and derives the realtime (wss://) host from it.
  map_events_endpoint = aws_appsync_api.map_events.dns["HTTP"]
}
