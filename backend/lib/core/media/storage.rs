//! Low-level, generic S3 operations

use std::sync::Arc;
use std::time::Duration;

use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

const UPLOAD_TTL: Duration = Duration::from_secs(300);
/// Coarse anti-abuse ceiling
const MAX_FETCH_BYTES: i64 = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct MediaState(Option<Arc<Inner>>);

struct Inner {
    client: Client,
    bucket: String,
}

impl MediaState {
    #[cfg(test)]
    pub fn disabled() -> Self {
        Self(None)
    }

    /// `MEDIA_BUCKET` unset means no client: routes answer
    /// `media_unavailable` instead of calling AWS.
    pub async fn from_env() -> Self {
        let bucket = std::env::var("MEDIA_BUCKET").ok().filter(|v| !v.is_empty());
        let Some(bucket) = bucket else {
            tracing::warn!("MEDIA_BUCKET not set: uploads disabled");
            return Self(None);
        };
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        Self(Some(Arc::new(Inner {
            client: Client::new(&config),
            bucket,
        })))
    }

    fn inner(&self) -> Result<&Inner, MediaError> {
        self.0.as_deref().ok_or(MediaError::Unavailable)
    }

    pub(super) async fn presign_put(
        &self,
        key: &str,
        content_type: &str,
    ) -> Result<String, MediaError> {
        let inner = self.inner()?;
        let presigned = inner
            .client
            .put_object()
            .bucket(&inner.bucket)
            .key(key)
            .content_type(content_type)
            .presigned(
                aws_sdk_s3::presigning::PresigningConfig::expires_in(UPLOAD_TTL)
                    .map_err(|err| MediaError::Upstream(err.to_string()))?,
            )
            .await
            .map_err(upstream)?;
        Ok(presigned.uri().to_string())
    }

    pub(super) async fn head_content_type(&self, key: &str) -> Result<String, MediaError> {
        let inner = self.inner()?;
        let head = inner
            .client
            .head_object()
            .bucket(&inner.bucket)
            .key(key)
            .send()
            .await
            .map_err(not_found_or_upstream)?;
        Ok(head
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string())
    }

    pub(super) async fn fetch(&self, key: &str) -> Result<(Vec<u8>, String), MediaError> {
        let inner = self.inner()?;

        let head = inner
            .client
            .head_object()
            .bucket(&inner.bucket)
            .key(key)
            .send()
            .await
            .map_err(not_found_or_upstream)?;
        if head.content_length().unwrap_or(0) > MAX_FETCH_BYTES {
            return Err(MediaError::TooLarge);
        }
        let content_type = head
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        let object = inner
            .client
            .get_object()
            .bucket(&inner.bucket)
            .key(key)
            .send()
            .await
            .map_err(upstream)?;
        let bytes =
            object.body.collect().await.map_err(|err| {
                MediaError::Upstream(format!("could not read object {key}: {err}"))
            })?;
        Ok((bytes.into_bytes().to_vec(), content_type))
    }

    pub(crate) async fn save(
        &self,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<(), MediaError> {
        let inner = self.inner()?;
        inner
            .client
            .put_object()
            .bucket(&inner.bucket)
            .key(key)
            .content_type(content_type)
            .body(ByteStream::from(bytes))
            .send()
            .await
            .map_err(upstream)?;
        Ok(())
    }

    pub(super) async fn copy(&self, from_key: &str, to_key: &str) -> Result<(), MediaError> {
        let inner = self.inner()?;
        inner
            .client
            .copy_object()
            .bucket(&inner.bucket)
            .copy_source(format!("{}/{from_key}", inner.bucket))
            .key(to_key)
            .send()
            .await
            .map_err(not_found_or_upstream)?;
        Ok(())
    }

    pub(crate) async fn delete(&self, key: &str) {
        let Ok(inner) = self.inner() else { return };
        if let Err(err) = inner
            .client
            .delete_object()
            .bucket(&inner.bucket)
            .key(key)
            .send()
            .await
        {
            tracing::warn!("media: could not delete stale object {key}: {err}");
        }
    }
}

/// A missing key surfaces under different codes depending on the operation
/// (head/get: `NotFound`/`NoSuchKey`; copy's source: `NoSuchKey`) -- checked
/// via the metadata every generated S3 error type carries, rather than one
/// operation-specific `is_not_found()` that wouldn't be generic across them.
fn not_found_or_upstream<E, R>(err: aws_sdk_s3::error::SdkError<E, R>) -> MediaError
where
    E: aws_sdk_s3::error::ProvideErrorMetadata + std::error::Error + 'static,
    R: std::fmt::Debug,
{
    let is_not_found = err
        .as_service_error()
        .and_then(|service_err| service_err.code())
        .is_some_and(|code| code == "NotFound" || code == "NoSuchKey");
    if is_not_found {
        MediaError::NotFound
    } else {
        upstream(err)
    }
}

fn upstream<E, R>(err: aws_sdk_s3::error::SdkError<E, R>) -> MediaError
where
    E: std::error::Error + 'static,
    R: std::fmt::Debug,
{
    MediaError::Upstream(
        aws_sdk_s3::error::DisplayErrorContext(&err)
            .to_string()
            .replace('\n', " "),
    )
}

#[derive(Debug)]
pub enum MediaError {
    /// No S3 client was built.
    Unavailable,
    /// The given key names no object.
    NotFound,
    /// The fetched object is bigger than [`MAX_FETCH_BYTES`].
    TooLarge,
    /// Anything the S3 API itself rejected.
    Upstream(String),
}

impl IntoResponse for MediaError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "media_unavailable",
                "this API instance has no media storage configured".to_string(),
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "upload_not_found",
                "no object was uploaded to that key".to_string(),
            ),
            Self::TooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "upload_too_large",
                "uploaded object exceeds the size limit".to_string(),
            ),
            Self::Upstream(why) => {
                tracing::error!("media: s3 call failed: {why}");
                (
                    StatusCode::BAD_GATEWAY,
                    "media_error",
                    "the storage backend rejected the call".to_string(),
                )
            }
        };
        (status, Json(json!({ "error": error, "message": message }))).into_response()
    }
}
