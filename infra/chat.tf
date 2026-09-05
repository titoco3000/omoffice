# ---------------------------------------------------------------------------
# Chat's WebSocket API.
#
# The HTTP API in main.tf (`protocol_type = "HTTP"`) cannot proxy a WebSocket
# handshake at all -- there is no upgrade path through it, ever. A WebSocket
# API is a genuinely different API Gateway resource, with its own event shape
# and its own routing: `$connect` once per socket opened, `$disconnect` once
# per socket closed, `$default` for everything else (a chat message, here --
# nothing sends a custom "action", so every message lands on `$default`).
#
# It points at the *same* lambda as the HTTP API. One binary, one function;
# `backend/src/chat/mod.rs`'s `lambda_entry` is what tells a WebSocket-API
# event apart from an HTTP-API request and dispatches accordingly -- see
# that module for why (API Gateway holds the actual connection open, not the
# lambda, so this is not a second copy of the HTTP server).
#
# Pushing a message back out to a *different* connection than the one that
# invoked the lambda goes through the Management API instead
# (`core::apigw_management`), which is what `execute-api:ManageConnections`
# below grants.
# ---------------------------------------------------------------------------

resource "aws_apigatewayv2_api" "chat" {
  name          = "${local.api_name}-chat"
  protocol_type = "WEBSOCKET"
  # Required for a WEBSOCKET api. There are no custom actions -- a request
  # with no matching route (i.e. every message) falls through to $default.
  route_selection_expression = "$request.body.action"
}

resource "aws_apigatewayv2_integration" "chat" {
  api_id           = aws_apigatewayv2_api.chat.id
  integration_type = "AWS_PROXY"
  integration_uri  = aws_lambda_function.api.invoke_arn
}

resource "aws_apigatewayv2_route" "chat_connect" {
  api_id    = aws_apigatewayv2_api.chat.id
  route_key = "$connect"
  target    = "integrations/${aws_apigatewayv2_integration.chat.id}"
}

resource "aws_apigatewayv2_route" "chat_disconnect" {
  api_id    = aws_apigatewayv2_api.chat.id
  route_key = "$disconnect"
  target    = "integrations/${aws_apigatewayv2_integration.chat.id}"
}

resource "aws_apigatewayv2_route" "chat_default" {
  api_id    = aws_apigatewayv2_api.chat.id
  route_key = "$default"
  target    = "integrations/${aws_apigatewayv2_integration.chat.id}"
}

resource "aws_apigatewayv2_stage" "chat" {
  api_id      = aws_apigatewayv2_api.chat.id
  name        = var.stage
  auto_deploy = true

  # No access_log_settings, on purpose: that needs an IAM role set as an
  # account-and-region-wide API Gateway setting (not scoped to this API),
  # which AWS checks at CreateStage time regardless of which API asks. This
  # is a "test" feature's WebSocket API -- CloudWatch's own default lambda
  # logs (main.tf's aws_cloudwatch_log_group.lambda) already cover
  # everything that matters for it, so it isn't worth turning this project's
  # per-stage Terraform workspaces (dev/prod, separate state each) into
  # something that has to coordinate a single account-wide resource between
  # them just for a second, more detailed log stream.
}

resource "aws_lambda_permission" "chat_api_gateway" {
  statement_id  = "AllowExecutionFromChatApiGateway${local.suffix}"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.api.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_apigatewayv2_api.chat.execution_arn}/*/*"
}

locals {
  # https://{api-id}.execute-api.{region}.amazonaws.com/{stage} -- the plain
  # HTTPS base URL `core::apigw_management::ApiGatewayManagement` posts
  # messages back through. Only the lambda ever calls this one.
  chat_management_endpoint = "https://${aws_apigatewayv2_api.chat.id}.execute-api.${var.region}.amazonaws.com/${aws_apigatewayv2_stage.chat.name}"
  # wss:// -- what a browser actually opens a socket against
  # (`?token=...&org_id=...`, see `chat/mod.rs`'s `lambda_entry` doc).
  chat_ws_url = "wss://${aws_apigatewayv2_api.chat.id}.execute-api.${var.region}.amazonaws.com/${aws_apigatewayv2_stage.chat.name}"
}

# ---------------------------------------------------------------------------
# Let the API lambda push messages back out through the WebSocket API above.
#
# Unlike Chime's calls (chime.tf), this one *is* resource-scopable, and is
# scoped down to this API's own connections in this stage -- there is no
# reason the chat lambda should be able to call PostToConnection against any
# other WebSocket API in the account.
# ---------------------------------------------------------------------------

data "aws_iam_policy_document" "api_chat_management" {
  statement {
    sid       = "PushChatMessages"
    actions   = ["execute-api:ManageConnections"]
    resources = ["${aws_apigatewayv2_api.chat.execution_arn}/${aws_apigatewayv2_stage.chat.name}/POST/@connections/*"]
  }
}

resource "aws_iam_role_policy" "api_chat_management" {
  name   = "${local.lambda_name}-chat-management"
  role   = aws_iam_role.lambda.id
  policy = data.aws_iam_policy_document.api_chat_management.json
}
