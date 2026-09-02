"""Secrets Manager rotation for the API's JWT signing key.

The secret is a single opaque HMAC string with no external system to update, so
the four rotation steps collapse to two pieces of real work: mint a new random
key as AWSPENDING (createSecret), then promote it to AWSCURRENT (finishSecret).
The key it replaces stays available as AWSPREVIOUS, which is what lets tokens
issued before the rotation keep working until they expire.
"""

import logging
import os

import boto3

logger = logging.getLogger()
logger.setLevel(logging.INFO)

client = boto3.client("secretsmanager")

KEY_LENGTH = int(os.environ.get("KEY_LENGTH", "64"))


def lambda_handler(event, context):
    arn = event["SecretId"]
    token = event["ClientRequestToken"]
    step = event["Step"]

    metadata = client.describe_secret(SecretId=arn)
    if not metadata.get("RotationEnabled"):
        raise ValueError(f"secret {arn} is not rotation-enabled")

    versions = metadata["VersionIdsToStages"]
    if token not in versions:
        raise ValueError(f"version {token} has no stage for secret {arn}")
    if "AWSCURRENT" in versions[token]:
        logger.info("version %s is already AWSCURRENT, nothing to do", token)
        return
    if "AWSPENDING" not in versions[token]:
        raise ValueError(f"version {token} is not AWSPENDING for secret {arn}")

    if step == "createSecret":
        create_secret(arn, token)
    elif step in ("setSecret", "testSecret"):
        # Nothing to configure or probe: the key is only ever read by our lambda.
        logger.info("step %s is a no-op for this secret", step)
    elif step == "finishSecret":
        finish_secret(arn, token)
    else:
        raise ValueError(f"unknown rotation step {step}")


def create_secret(arn, token):
    """Put a fresh random key in AWSPENDING, unless this token already has one."""
    try:
        client.get_secret_value(SecretId=arn, VersionId=token, VersionStage="AWSPENDING")
        logger.info("AWSPENDING already exists for version %s", token)
        return
    except client.exceptions.ResourceNotFoundException:
        pass

    key = client.get_random_password(
        PasswordLength=KEY_LENGTH,
        ExcludePunctuation=True,
        RequireEachIncludedType=True,
    )["RandomPassword"]

    client.put_secret_value(
        SecretId=arn,
        ClientRequestToken=token,
        SecretString=key,
        VersionStages=["AWSPENDING"],
    )
    logger.info("created new AWSPENDING key for version %s", token)


def finish_secret(arn, token):
    """Move AWSCURRENT onto the pending version; the old one becomes AWSPREVIOUS."""
    metadata = client.describe_secret(SecretId=arn)

    current_version = None
    for version, stages in metadata["VersionIdsToStages"].items():
        if "AWSCURRENT" in stages:
            if version == token:
                logger.info("version %s is already AWSCURRENT", token)
                return
            current_version = version
            break

    client.update_secret_version_stage(
        SecretId=arn,
        VersionStage="AWSCURRENT",
        MoveToVersionId=token,
        RemoveFromVersionId=current_version,
    )
    logger.info("promoted version %s to AWSCURRENT", token)
