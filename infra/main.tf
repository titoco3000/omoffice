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

# Trusts both lambda and ecs-tasks so this role can be reused as the new
# ECS task role during the phase-14 Fargate migration (dev-docs/phase/14.md)
# without touching the 5 aws_iam_role_policy attachments that already
# reference it (dsql.tf, chime.tf, media.tf, messenger.tf, secrets.tf).
# Narrow back to ecs-tasks only once aws_lambda_function.api is deleted.
data "aws_iam_policy_document" "lambda_assume" {
  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }
  }

  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["ecs-tasks.amazonaws.com"]
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
      # Shared secret POST /chime/events checks the EventBridge API
      # destination's header against -- see chime.tf.
      CHIME_WEBHOOK_SECRET = random_password.chime_webhook_secret.result
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

# ---------------------------------------------------------------------------
# ECS Fargate + ALB (dev-docs/phase/14.md)
#
# Stood up alongside the still-live lambda + both apigatewayv2 API sets
# (this file's above, and infra/messenger.tf's) rather than replacing them:
# deleting the lambda now would strand messenger.tf's integration, which
# points at aws_lambda_function.api.invoke_arn. Traffic cuts over to the ALB
# only once it's verified directly (curl, websocket, a real Chime meeting);
# the old stack is deleted in a later, separate pass once that cutover has
# soaked. See dev-docs/phase/14.md and backend/Makefile's `deploy` target.
#
# No VPC/subnet/ALB/ECS resources exist anywhere in this stack before this
# block -- DSQL was chosen specifically to keep the lambda out of a VPC (see
# dev-docs/phase/0 - pre-overhaul/6.md), so this starts from the account's
# default VPC and its public subnets rather than a purpose-built network.
# ---------------------------------------------------------------------------

data "aws_vpc" "default" {
  default = true
}

data "aws_subnets" "default" {
  filter {
    name   = "vpc-id"
    values = [data.aws_vpc.default.id]
  }
}

resource "aws_ecr_repository" "api" {
  name = local.lambda_name
  # Dev needs a clean `terraform destroy`; prod never gets torn down casually.
  force_delete = var.stage != "prod"
}

resource "aws_ecs_cluster" "main" {
  name = local.api_name
}

data "aws_iam_policy_document" "ecs_tasks_assume" {
  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["ecs-tasks.amazonaws.com"]
    }
  }
}

# Pulls the image and writes container logs -- distinct from aws_iam_role.lambda,
# which is the *task* role (app-level AWS permissions: DSQL, Chime, media, JWT
# secret) reused as-is from the lambda era, see the widened assume policy above.
resource "aws_iam_role" "ecs_execution" {
  name               = "${var.project}-ecs-execution${local.suffix}"
  assume_role_policy = data.aws_iam_policy_document.ecs_tasks_assume.json
}

resource "aws_iam_role_policy_attachment" "ecs_execution" {
  role       = aws_iam_role.ecs_execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_cloudwatch_log_group" "ecs" {
  name              = "/ecs/${local.lambda_name}"
  retention_in_days = var.log_retention_days
}

# The image is pushed out-of-band by `cd backend && make deploy <stage>`, not
# by terraform -- same reason aws_lambda_function.api above ignores
# filename/source_code_hash. The ECR repo starts empty, so the ECS service
# below stays unhealthy until that first `make deploy` runs.
resource "aws_ecs_task_definition" "api" {
  family                   = local.lambda_name
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"
  execution_role_arn       = aws_iam_role.ecs_execution.arn
  task_role_arn            = aws_iam_role.lambda.arn

  runtime_platform {
    cpu_architecture        = "ARM64"
    operating_system_family = "LINUX"
  }

  container_definitions = jsonencode([
    {
      name  = local.lambda_name
      image = "${aws_ecr_repository.api.repository_url}:${var.stage}"
      portMappings = [
        { containerPort = 8000, protocol = "tcp" }
      ]
      # ECS sends SIGTERM then waits this long before SIGKILL -- gives
      # backend/main.rs's graceful-shutdown handler time to flush every
      # in-memory map position to map_positions before the task exits.
      stopTimeout = 30
      environment = [
        { name = "STAGE", value = var.stage },
        { name = "RUST_LOG", value = "info" },
        { name = "RUST_BACKTRACE", value = "1" },
        { name = "PORT", value = "8000" },
        { name = "JWT_SECRET_ID", value = aws_secretsmanager_secret.jwt.name },
        { name = "DATABASE_ENDPOINT", value = local.db_endpoint },
        { name = "MEDIA_BUCKET", value = aws_s3_bucket.media.bucket },
        { name = "CHIME_WEBHOOK_SECRET", value = random_password.chime_webhook_secret.result },
      ]
      logConfiguration = {
        logDriver = "awslogs"
        options = {
          "awslogs-group"         = aws_cloudwatch_log_group.ecs.name
          "awslogs-region"        = var.region
          "awslogs-stream-prefix" = "api"
        }
      }
    }
  ])

  lifecycle {
    ignore_changes = [container_definitions]
  }
}

resource "aws_security_group" "alb" {
  name        = "${local.api_name}-alb"
  description = "Inbound HTTP from the internet to the ALB."
  vpc_id      = data.aws_vpc.default.id

  ingress {
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_security_group" "ecs_task" {
  name        = "${local.api_name}-ecs-task"
  description = "Inbound only from the ALB; no NAT, so egress is open (DSQL/Chime/S3/Secrets Manager/ECR all sit outside the VPC)."
  vpc_id      = data.aws_vpc.default.id

  ingress {
    from_port       = 8000
    to_port         = 8000
    protocol        = "tcp"
    security_groups = [aws_security_group.alb.id]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_lb" "api" {
  name               = local.api_name
  internal           = false
  load_balancer_type = "application"
  security_groups    = [aws_security_group.alb.id]
  subnets            = data.aws_subnets.default.ids
}

resource "aws_lb_target_group" "api" {
  name        = local.api_name
  port        = 8000
  protocol    = "HTTP"
  vpc_id      = data.aws_vpc.default.id
  target_type = "ip" # required for Fargate awsvpc networking

  health_check {
    path                = "/health"
    matcher             = "200"
    interval            = 30
    timeout             = 5
    healthy_threshold   = 2
    unhealthy_threshold = 3
  }
}

# No ACM cert/custom domain exists anywhere in this stack yet -- HTTP only
# for now, same known gap called out in dev-docs/phase/14.md's write-up.
resource "aws_lb_listener" "api" {
  load_balancer_arn = aws_lb.api.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.api.arn
  }
}

resource "aws_ecs_service" "api" {
  name            = local.lambda_name
  cluster         = aws_ecs_cluster.main.id
  task_definition = aws_ecs_task_definition.api.arn
  launch_type     = "FARGATE"

  # Hardcoded, not a variable: core::communication::web_messenger's
  # local_web_socket transport is an in-process HashMap of open sockets for
  # the life of the process (see that module's doc). A second replica would
  # hold a disjoint half of connections and silently break delivery -- do
  # not raise this without first giving the websocket transport a shared
  # backplane.
  desired_count = 1

  # ECS's default rolling deployment (100/200) would briefly run two tasks
  # during every deploy, which breaks the same single-process invariant as
  # desired_count > 1 would. 0/100 makes every deploy stop-then-start
  # instead: a brief connection-refused window, not split-brain.
  deployment_minimum_healthy_percent = 0
  deployment_maximum_percent         = 100

  network_configuration {
    subnets          = data.aws_subnets.default.ids
    security_groups  = [aws_security_group.ecs_task.id]
    assign_public_ip = true
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.api.arn
    container_name   = local.lambda_name
    container_port   = 8000
  }

  depends_on = [aws_lb_listener.api]
}
