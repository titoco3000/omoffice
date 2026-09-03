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
