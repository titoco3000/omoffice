//! The org's editable tile map: one PNG per org at `maps/{org_id}.png` in
//! the media bucket, `LEVELS` strips of `OFFICE_SIZE` x `OFFICE_SIZE` side
//! by side (base, mid, top -- see the frontend's `tileMap.js`). An org that
//! never saved one has no object, and its frontend falls back to the map
//! bundled with it.
//!
//! Saving reuses the per-user staging slot (`core::media`): the browser PUTs
//! the edited PNG to a presigned URL, then [`confirm_route`] promotes it here
//! -- resized to the right size if it isn't already -- and tells every
//! online member's frontend to fetch it again over [`UPDATED_CHANNEL`].

use axum::extract::{Extension, Path};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use serde_json::json;

use super::{MapError, OFFICE_SIZE};
use crate::core::auth::Claims;
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::Db;
use crate::core::media::MediaState;
use crate::core::rbac;

/// Base, mid and top.
const LEVELS: u32 = 3;
const MAP_WIDTH: u32 = OFFICE_SIZE as u32 * LEVELS;
const MAP_HEIGHT: u32 = OFFICE_SIZE as u32;

/// `{"org_id"}`, sent to every online member of the org once its map has
/// been replaced.
pub const UPDATED_CHANNEL: web_messenger::Channel = "map_updated";

fn map_key(org_id: &str) -> String {
    format!("maps/{org_id}.png")
}

/// `image` as an exactly `MAP_WIDTH` x `MAP_HEIGHT` PNG. Resampled with
/// nearest-neighbor when the size is off: every pixel is a color code, so
/// blending neighbors would invent codes that mean something else.
fn normalize(image: DynamicImage) -> Result<Vec<u8>, MapError> {
    let image = if image.dimensions() == (MAP_WIDTH, MAP_HEIGHT) {
        image
    } else {
        image.resize_exact(MAP_WIDTH, MAP_HEIGHT, FilterType::Nearest)
    };
    let mut png = Vec::new();
    DynamicImage::ImageRgba8(image.to_rgba8())
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|_| MapError::InvalidImage)?;
    Ok(png)
}

/// `GET /map/org/{org_id}/tilemap` -- the org's saved map as a PNG, or 404
/// (`upload_not_found`) when it has none.
pub(super) async fn get_route(
    Extension(db): Extension<Db>,
    Extension(media): Extension<MediaState>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Response, MapError> {
    rbac::require_member(&db, &org_id, &claims.sub).await?;
    let (bytes, _) = media.fetch(&map_key(&org_id)).await?;
    // Never cached: the frontend refetches on `UPDATED_CHANNEL` and must not
    // be handed the previous map.
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response())
}

/// `PUT /map/org/{org_id}/tilemap` -- replace the org's map with the
/// caller's staged upload.
pub(super) async fn confirm_route(
    Extension(db): Extension<Db>,
    Extension(media): Extension<MediaState>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<StatusCode, MapError> {
    rbac::require_member(&db, &org_id, &claims.sub).await?;

    let declared_type = media.get_staged_content_type(&claims.sub).await?;
    if !declared_type.starts_with("image/") {
        return Err(MapError::UnsupportedMediaType);
    }
    let (bytes, _) = media.get_staged(&claims.sub).await?;
    let image = image::load_from_memory(&bytes).map_err(|_| MapError::InvalidImage)?;
    media
        .save(&map_key(&org_id), normalize(image)?, "image/png")
        .await?;
    media.clear_staged(&claims.sub).await;

    let audience = rbac::online_member_ids(&db, &org_id).await?;
    messenger
        .send(UPDATED_CHANNEL, &audience, json!({ "org_id": org_id }))
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn decode(png: &[u8]) -> RgbaImage {
        image::load_from_memory(png).unwrap().to_rgba8()
    }

    #[test]
    fn right_sized_map_is_kept_pixel_for_pixel() {
        let mut map = RgbaImage::new(MAP_WIDTH, MAP_HEIGHT);
        map.put_pixel(7, 3, Rgba([255, 128, 0, 255]));
        let out = decode(&normalize(DynamicImage::ImageRgba8(map)).unwrap());
        assert_eq!(out.dimensions(), (MAP_WIDTH, MAP_HEIGHT));
        assert_eq!(out.get_pixel(7, 3), &Rgba([255, 128, 0, 255]));
        assert_eq!(out.get_pixel(8, 3), &Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn wrong_sized_map_is_resized_without_blending_colors() {
        let map = RgbaImage::from_pixel(30, 10, Rgba([0, 255, 0, 255]));
        let out = decode(&normalize(DynamicImage::ImageRgba8(map)).unwrap());
        assert_eq!(out.dimensions(), (MAP_WIDTH, MAP_HEIGHT));
        assert!(out.pixels().all(|p| p == &Rgba([0, 255, 0, 255])));
    }
}
