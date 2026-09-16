//! Avatar-specific image handling

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use uuid::Uuid;

use crate::core::media::MediaState;

use super::RbacError;

pub const MAX_AVATAR_SIZE: u32 = 256;

/// Promote staged upload to an avatar
pub(super) async fn promote(media: &MediaState, user_id: &str) -> Result<String, RbacError> {
    // checks for valid content type (cheap check)
    let declared_type = media.get_staged_content_type(user_id).await?;
    if !declared_type.starts_with("image/") {
        return Err(RbacError::UnsupportedMediaType);
    }

    // loads image and checks if valid
    let (bytes, _content_type) = media.get_staged(user_id).await?;
    let image = image::load_from_memory(&bytes).map_err(|_| RbacError::InvalidImage)?;

    // transforms image into required size
    let squared = square(image);
    let mut png = Vec::new();
    squared
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|_| RbacError::InvalidImage)?;

    // saves result
    let final_key = format!("avatars/{}.png", Uuid::new_v4());
    media.save(&final_key, png, "image/png").await?;

    media.clear_staged(user_id).await;
    Ok(final_key)
}

/// Center-crop to a square, then resample to MAX_AVATAR_SIZE
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
