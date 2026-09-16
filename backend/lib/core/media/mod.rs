mod staging;
mod storage;

pub use storage::{MediaError, MediaState};

use axum::{
    extract::{Extension, Query},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::core::auth::Claims;

#[derive(Deserialize)]
struct UploadParams {
    /// Declared up front so the presigned PUT can bind to it -- see
    /// [`MediaState::create_upload_link`].
    content_type: String,
}

/// `GET /media/upload` -- presign an S3 PUT into the caller's staging slot
async fn upload_route(
    Extension(media): Extension<MediaState>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<UploadParams>,
) -> Result<Json<Value>, MediaError> {
    let upload_url = media
        .create_upload_link(&claims.sub, &params.content_type)
        .await?;
    Ok(Json(json!({ "upload_url": upload_url })))
}

pub fn routes() -> Router {
    Router::new().route("/upload", get(upload_route))
}
