# ---------------------------------------------------------------------------
# Amazon Chime SDK meetings.
#
# There is nothing to create: a meeting is made on demand by the API lambda and
# lives only as long as someone is in it. All this file grants is the right to
# make those calls.
# ---------------------------------------------------------------------------

data "aws_iam_policy_document" "api_chime" {
  statement {
    sid = "RunChimeSdkMeetings"
    actions = [
      "chime:CreateMeeting",
      "chime:GetMeeting",
      "chime:DeleteMeeting",
      "chime:CreateAttendee",
      "chime:GetAttendee",
      "chime:DeleteAttendee",
      "chime:ListAttendees",
    ]
    # The Chime SDK meeting actions take no resource ARN: a meeting does not
    # exist yet when CreateMeeting is called, and none of them are
    # resource-scopable.
    resources = ["*"]
  }
}

resource "aws_iam_role_policy" "api_chime" {
  name   = "${local.lambda_name}-chime"
  role   = aws_iam_role.lambda.id
  policy = data.aws_iam_policy_document.api_chime.json
}

# ---------------------------------------------------------------------------
# EventBridge -> POST /chime/events -- Chime's own signal that a meeting
# emptied out and it dropped the room (see the module doc on
# backend/lib/core/chime.rs). The default event bus already carries these
# without any setup on Chime's side; this just points a rule at the API.
#
# An API destination, not a direct Lambda target: `lambda_http` (main.rs)
# only knows how to deserialize API Gateway/ALB/WebSocket event shapes, not
# a raw EventBridge invoke, so the notification has to arrive as a normal
# HTTP request through the same front door everything else uses. EventBridge
# still needs a role to assume for this ("who's allowed to invoke *this*
# destination", separate from the connection's own header-based auth on the
# far end) -- see chime_webhook's IAM role/policy below.
# ---------------------------------------------------------------------------

resource "random_password" "chime_webhook_secret" {
  length  = 32
  special = false
}

resource "aws_cloudwatch_event_connection" "chime_webhook" {
  name               = "${local.lambda_name}-chime-webhook"
  authorization_type = "API_KEY"

  auth_parameters {
    api_key {
      key   = "x-chime-webhook-secret"
      value = random_password.chime_webhook_secret.result
    }
  }
}

resource "aws_cloudwatch_event_api_destination" "chime_webhook" {
  name                = "${local.lambda_name}-chime-webhook"
  connection_arn      = aws_cloudwatch_event_connection.chime_webhook.arn
  invocation_endpoint = "${aws_apigatewayv2_stage.default.invoke_url}chime/events"
  http_method         = "POST"
}

resource "aws_cloudwatch_event_rule" "chime_meeting_ended" {
  name = "${local.lambda_name}-chime-meeting-ended"

  # Filtered to MeetingEnded here too, on top of the handler's own check --
  # AttendeeLeft fires on every departure, not just the one that empties a
  # room, and there's no reason to pay for an invocation it always no-ops on.
  event_pattern = jsonencode({
    source      = ["aws.chime"]
    detail-type = ["Chime Meeting State Change"]
    detail = {
      eventType = ["chime:MeetingEnded"]
    }
  })
}

data "aws_iam_policy_document" "chime_webhook_assume" {
  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["events.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "chime_webhook" {
  name               = "${local.lambda_name}-chime-webhook"
  assume_role_policy = data.aws_iam_policy_document.chime_webhook_assume.json
}

data "aws_iam_policy_document" "chime_webhook_invoke" {
  statement {
    sid       = "InvokeChimeWebhookDestination"
    actions   = ["events:InvokeApiDestination"]
    resources = [aws_cloudwatch_event_api_destination.chime_webhook.arn]
  }
}

resource "aws_iam_role_policy" "chime_webhook_invoke" {
  name   = "${local.lambda_name}-chime-webhook-invoke"
  role   = aws_iam_role.chime_webhook.id
  policy = data.aws_iam_policy_document.chime_webhook_invoke.json
}

resource "aws_cloudwatch_event_target" "chime_meeting_ended" {
  rule     = aws_cloudwatch_event_rule.chime_meeting_ended.name
  arn      = aws_cloudwatch_event_api_destination.chime_webhook.arn
  role_arn = aws_iam_role.chime_webhook.arn
}
