# ---------------------------------------------------------------------------
# User-uploaded media (avatars, org icons): a private S3 bucket the API
# lambda writes/reads/deletes directly, fronted by CloudFront for public
# reads. Same OAC split as frontend.tf's static bucket, minus the SPA
# fallback -- a missing key here really is missing, not a client route.
#
# Uploads never go through the lambda's own bandwidth: the API only ever
# hands out a presigned PUT URL (core::media::MediaState::presign_upload),
# and the browser PUTs the file straight to S3 -- hence the CORS rule below.
# ---------------------------------------------------------------------------

locals {
  media_bucket_name = "${var.project}-media${local.suffix}-${data.aws_caller_identity.current.account_id}"
}

resource "aws_s3_bucket" "media" {
  bucket = local.media_bucket_name

  # Sample project: let `make destroy` remove the bucket even with objects in it.
  force_destroy = true
}

resource "aws_s3_bucket_public_access_block" "media" {
  bucket                  = aws_s3_bucket.media.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_ownership_controls" "media" {
  bucket = aws_s3_bucket.media.id

  rule {
    object_ownership = "BucketOwnerEnforced"
  }
}

# A presigned upload URL only reserves a key -- a bearer token got you the
# URL, not a promise you'll ever call `/media/avatar/confirm`. Anything left
# under `uploads/` (core::media's staging prefix; confirmed pictures move to
# `avatars/` and are never touched by this rule) is either mid-upload or
# abandoned, so a day is generous and bounds how much of the bucket an
# unconfirmed-upload loop -- accidental or deliberate -- can ever occupy.
resource "aws_s3_bucket_lifecycle_configuration" "media" {
  bucket = aws_s3_bucket.media.id

  rule {
    id     = "expire-unconfirmed-uploads"
    status = "Enabled"

    filter {
      prefix = "uploads/"
    }

    expiration {
      days = 1
    }
  }
}

# The browser PUTs directly to a presigned URL, from whatever origin the
# frontend is served on (including localhost during development) -- so this
# stays wide open on origin rather than pinned to one stage's CloudFront
# domain.
resource "aws_s3_bucket_cors_configuration" "media" {
  bucket = aws_s3_bucket.media.id

  cors_rule {
    allowed_methods = ["PUT", "GET", "HEAD"]
    allowed_origins = ["*"]
    allowed_headers = ["*"]
    max_age_seconds = 3000
  }
}

resource "aws_cloudfront_origin_access_control" "media" {
  name                              = local.media_bucket_name
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

resource "aws_cloudfront_distribution" "media" {
  enabled     = true
  comment     = "${var.project} media (${var.stage})"
  price_class = "PriceClass_100"

  origin {
    domain_name              = aws_s3_bucket.media.bucket_regional_domain_name
    origin_id                = "s3-${local.media_bucket_name}"
    origin_access_control_id = aws_cloudfront_origin_access_control.media.id
  }

  default_cache_behavior {
    target_origin_id       = "s3-${local.media_bucket_name}"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD"]
    cached_methods         = ["GET", "HEAD"]

    # AWS managed policy: CachingOptimized, no forwarded headers/cookies.
    cache_policy_id = "658327ea-f89d-4fab-a63d-7e88639e58f6"
    compress        = true
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    cloudfront_default_certificate = true
  }
}

data "aws_iam_policy_document" "media_bucket" {
  statement {
    sid       = "AllowCloudFrontRead"
    actions   = ["s3:GetObject"]
    resources = ["${aws_s3_bucket.media.arn}/*"]

    principals {
      type        = "Service"
      identifiers = ["cloudfront.amazonaws.com"]
    }

    condition {
      test     = "StringEquals"
      variable = "AWS:SourceArn"
      values   = [aws_cloudfront_distribution.media.arn]
    }
  }
}

resource "aws_s3_bucket_policy" "media" {
  bucket     = aws_s3_bucket.media.id
  policy     = data.aws_iam_policy_document.media_bucket.json
  depends_on = [aws_s3_bucket_public_access_block.media]
}

# ---------------------------------------------------------------------------
# Rights for the API lambda: it presigns uploads, then fetches/normalizes/
# overwrites the object on confirm, and deletes whatever picture a
# replacement just made stale -- see core::media.
# ---------------------------------------------------------------------------

data "aws_iam_policy_document" "api_media" {
  statement {
    sid       = "ReadWriteMediaObjects"
    actions   = ["s3:GetObject", "s3:PutObject", "s3:DeleteObject"]
    resources = ["${aws_s3_bucket.media.arn}/*"]
  }
}

resource "aws_iam_role_policy" "api_media" {
  name   = "${local.lambda_name}-media"
  role   = aws_iam_role.lambda.id
  policy = data.aws_iam_policy_document.api_media.json
}
