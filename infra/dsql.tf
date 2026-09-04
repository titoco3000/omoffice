# ---------------------------------------------------------------------------
# Aurora DSQL — the SQL database, one cluster per stage.
#
# DSQL is serverless, scales to zero, and has a public endpoint, so the API
# lambda stays out of a VPC and the stack needs no NAT gateway. It authenticates
# with short-lived IAM tokens instead of a password, so unlike the JWT key there
# is no secret here to store or rotate.
#
# Terraform owns the cluster; the schema inside it is owned by the migration
# runner in backend/ (`make db-migrate <stage>`), the same split as the lambda
# and its code.
# ---------------------------------------------------------------------------

locals {
  db_name = "${var.project}-db${local.suffix}"

  # DSQL publishes no endpoint attribute; it is derived from the cluster id.
  db_endpoint = "${aws_dsql_cluster.main.identifier}.dsql.${var.region}.on.aws"
}

resource "aws_dsql_cluster" "main" {
  # prod keeps the guard rail; dev is meant to be torn down and rebuilt.
  deletion_protection_enabled = var.stage == "prod"
  force_destroy               = var.stage != "prod"

  tags = {
    Name = local.db_name
  }
}

# ---------------------------------------------------------------------------
# Connect rights for the API lambda
#
# The API connects as the built-in `admin` database role, which is what
# DbConnectAdmin grants. A non-admin role would mean creating database roles
# inside the cluster and mapping them to IAM — more machinery than a sample
# with one caller needs.
# ---------------------------------------------------------------------------

data "aws_iam_policy_document" "api_dsql" {
  statement {
    sid       = "ConnectToDsqlAsAdmin"
    actions   = ["dsql:DbConnectAdmin"]
    resources = [aws_dsql_cluster.main.arn]
  }
}

resource "aws_iam_role_policy" "api_dsql" {
  name   = "${local.lambda_name}-dsql"
  role   = aws_iam_role.lambda.id
  policy = data.aws_iam_policy_document.api_dsql.json
}
