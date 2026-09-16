use std::sync::Arc;
use std::time::Duration;

use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use axum::{
    extract::Extension,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, post},
    Json, Router,
};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::core::auth::Claims;
use crate::core::communication::web_messenger::Messenger;
use crate::core::data::db::Db;
use crate::core::rbac::{self, roles, RbacError};

pub const MAX_AVATAR_SIZE: u32 = 256;

const UPLOAD_TTL: Duration = Duration::from_secs(300);
/// decompression-bomb guard
const MAX_UPLOAD_BYTES: i64 = 8 * 1024 * 1024;

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
            tracing::warn!("MEDIA_BUCKET not set: avatar uploads disabled");
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

    async fn presign_upload(&self, db: &Db, user_id: &str) -> Result<(String, String), MediaError> {
        let inner = self.inner()?;
        let key = staging_key(user_id);

        if rbac::upload_pending(db, user_id).await? {
            self.delete(&key).await;
        }
        rbac::mark_upload_pending(db, user_id).await?;

        let presigned = inner
            .client
            .put_object()
            .bucket(&inner.bucket)
            .key(&key)
            .presigned(
                aws_sdk_s3::presigning::PresigningConfig::expires_in(UPLOAD_TTL)
                    .map_err(|err| MediaError::Upstream(err.to_string()))?,
            )
            .await
            .map_err(upstream)?;
        Ok((key, presigned.uri().to_string()))
    }

    /// Fetch whatever a client just PUT to the `uploads/` staging `key`,
    /// decode it, force it down to an exact [`MAX_AVATAR_SIZE`] square
    /// (center-cropped, Lanczos3 resampled), store the result under a fresh
    /// `avatars/` key, and delete the staging object. Returns the
    /// permanent key.
    async fn promote(&self, staging_key: &str) -> Result<String, MediaError> {
        let inner = self.inner()?;

        let head = inner
            .client
            .head_object()
            .bucket(&inner.bucket)
            .key(staging_key)
            .send()
            .await
            .map_err(|err| {
                if err.as_service_error().is_some_and(|e| e.is_not_found()) {
                    MediaError::NotFound
                } else {
                    upstream(err)
                }
            })?;
        if head.content_length().unwrap_or(0) > MAX_UPLOAD_BYTES {
            return Err(MediaError::TooLarge);
        }

        let object = inner
            .client
            .get_object()
            .bucket(&inner.bucket)
            .key(staging_key)
            .send()
            .await
            .map_err(upstream)?;
        let bytes = object.body.collect().await.map_err(|err| {
            MediaError::Upstream(format!("could not read uploaded object: {err}"))
        })?;

        let image =
            image::load_from_memory(&bytes.into_bytes()).map_err(|_| MediaError::InvalidImage)?;
        let squared = square(image);

        let mut png = Vec::new();
        squared
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|err| MediaError::Upstream(format!("could not encode png: {err}")))?;

        let final_key = format!("avatars/{}.png", Uuid::new_v4());
        inner
            .client
            .put_object()
            .bucket(&inner.bucket)
            .key(&final_key)
            .content_type("image/png")
            .body(ByteStream::from(png))
            .send()
            .await
            .map_err(upstream)?;

        // Best-effort: the lifecycle rule on `uploads/` (infra/media.tf)
        // catches this anyway if the delete itself fails.
        self.delete(staging_key).await;
        Ok(final_key)
    }

    /// Best-effort delete of a picture a caller just replaced -- logged,
    /// never failed on: the row that named it has already moved to the new
    /// key, so there is no request left to fail this back to.
    async fn delete(&self, key: &str) {
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

/// Where user's upload stays before beeing promoted
fn staging_key(user_id: &str) -> String {
    format!("uploads/{user_id}")
}

/// Center-crop to a square (the shorter side), then resample to exactly
/// `MAX_AVATAR_SIZE` x `MAX_AVATAR_SIZE`.
fn square(image: DynamicImage) -> DynamicImage {
    let (width, height) = image.dimensions();
    let side = width.min(height);
    let x = (width - side) / 2;
    let y = (height - side) / 2;
    image.crop_imm(x, y, side, side).resize_exact(
        MAX_AVATAR_SIZE,
        MAX_AVATAR_SIZE,
        FilterType::Lanczos3,
    )
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
    /// The presigned key names no uploaded object.
    NotFound,
    /// The uploaded object is bigger than [`MAX_UPLOAD_BYTES`].
    TooLarge,
    /// The uploaded object could not be decoded as an image.
    InvalidImage,
    /// Anything the S3 API itself rejected.
    Upstream(String),
    Rbac(RbacError),
}

impl From<RbacError> for MediaError {
    fn from(err: RbacError) -> Self {
        Self::Rbac(err)
    }
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
                "image_too_large",
                "uploaded image exceeds the size limit".to_string(),
            ),
            Self::InvalidImage => (
                StatusCode::BAD_REQUEST,
                "invalid_image",
                "uploaded object is not a decodable image".to_string(),
            ),
            Self::Upstream(why) => {
                tracing::error!("media: s3 call failed: {why}");
                (
                    StatusCode::BAD_GATEWAY,
                    "media_error",
                    "the storage backend rejected the call".to_string(),
                )
            }
            Self::Rbac(err) => return err.into_response(),
        };
        (status, Json(json!({ "error": error, "message": message }))).into_response()
    }
}

async fn upload_url_route(
    Extension(media): Extension<MediaState>,
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, MediaError> {
    let (key, upload_url) = media.presign_upload(&db, &claims.sub).await?;
    Ok(Json(json!({ "key": key, "upload_url": upload_url })))
}

/// Which `picture` column [`confirm_route`] assigns the normalized key to.
/// `Me` is always allowed; `OrgMe` requires membership in `org_id`;
/// `OrgIcon` requires `admin` in `org_id` (same gate as renaming the org).
#[derive(Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
enum Target {
    Me,
    OrgMe { org_id: String },
    OrgIcon { org_id: String },
}

async fn confirm_route(
    Extension(media): Extension<MediaState>,
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Json(target): Json<Target>,
) -> Result<Json<Value>, MediaError> {
    // Never the client's word for which staging object is theirs -- the key
    // is deterministic (see `presign_upload`), so it's derived from the
    // caller's own claims instead of trusted off the request body.
    let final_key = media.promote(&staging_key(&claims.sub)).await?;
    // Staging is now provably empty for this user -- clear the flag rather
    // than waiting for the next `presign_upload` to notice.
    rbac::clear_upload_pending(&db, &claims.sub).await?;

    // Old key to clean up once the new one is safely assigned, mirroring
    // the rename routes' "assign, then broadcast" order -- see the module
    // doc for why deletion is best-effort rather than part of this result.
    let previous = match &target {
        Target::Me => {
            let previous = rbac::get_user(&db, &claims.sub)
                .await?
                .and_then(|user| user.picture);
            rbac::set_picture_globally(&db, &claims.sub, Some(&final_key)).await?;
            // Same fan-out as renaming globally: every org this user
            // belongs to may show the new picture, not just whichever is
            // open right now.
            for org in rbac::list_orgs_for_user(&db, &claims.sub).await? {
                rbac::broadcast_roster(&messenger, &db, &org.id).await?;
            }
            previous
        }
        Target::OrgMe { org_id } => {
            rbac::require_member(&db, org_id, &claims.sub).await?;
            let previous = rbac::member_picture(&db, org_id, &claims.sub).await?;
            rbac::set_picture_in_org(&db, org_id, &claims.sub, Some(&final_key)).await?;
            rbac::broadcast_roster(&messenger, &db, org_id).await?;
            previous
        }
        Target::OrgIcon { org_id } => {
            rbac::require_role(&db, org_id, &claims.sub, roles::ADMIN).await?;
            let previous = rbac::org_picture(&db, org_id).await?;
            rbac::set_org_picture(&db, org_id, Some(&final_key)).await?;
            previous
        }
    };

    if let Some(previous) = previous {
        media.delete(&previous).await;
    }

    Ok(Json(json!({ "key": final_key })))
}

/// Clears whichever `picture` column `target` names, back to no picture at
/// all (rather than a specific key) -- unlike [`confirm_route`], this needs
/// no staged upload and works even with [`MediaState::Unavailable`], since
/// deleting the old S3 object is best-effort (see [`MediaState::delete`])
/// and clearing the column is the only part that must succeed.
///
/// No-op (no write, no broadcast) when there was nothing to remove --
/// clicking "remove photo" twice, or a stale tab replaying the request,
/// should not push a roster update for a picture that isn't actually
/// changing.
async fn remove_route(
    Extension(media): Extension<MediaState>,
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Json(target): Json<Target>,
) -> Result<Json<Value>, MediaError> {
    let previous = match &target {
        Target::Me => {
            let previous = rbac::get_user(&db, &claims.sub)
                .await?
                .and_then(|user| user.picture);
            if previous.is_some() {
                rbac::set_picture_globally(&db, &claims.sub, None).await?;
                for org in rbac::list_orgs_for_user(&db, &claims.sub).await? {
                    rbac::broadcast_roster(&messenger, &db, &org.id).await?;
                }
            }
            previous
        }
        Target::OrgMe { org_id } => {
            rbac::require_member(&db, org_id, &claims.sub).await?;
            let previous = rbac::member_picture(&db, org_id, &claims.sub).await?;
            if previous.is_some() {
                rbac::set_picture_in_org(&db, org_id, &claims.sub, None).await?;
                rbac::broadcast_roster(&messenger, &db, org_id).await?;
            }
            previous
        }
        Target::OrgIcon { org_id } => {
            rbac::require_role(&db, org_id, &claims.sub, roles::ADMIN).await?;
            let previous = rbac::org_picture(&db, org_id).await?;
            if previous.is_some() {
                rbac::set_org_picture(&db, org_id, None).await?;
            }
            previous
        }
    };

    if let Some(previous) = previous {
        media.delete(&previous).await;
    }

    Ok(Json(json!({ "key": null })))
}

/// `/media/*` -- nested under [`auth::require_bearer`](crate::core::auth::require_bearer).
pub fn routes() -> Router {
    Router::new()
        .route("/avatar/upload-url", post(upload_url_route))
        .route("/avatar/confirm", post(confirm_route))
        .route("/avatar", delete(remove_route))
}
