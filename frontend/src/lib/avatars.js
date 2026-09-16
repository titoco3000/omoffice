//! Avatar/org-icon upload: calls into `/media/*`, the contract
//! `core::media::routes` mounts (backend/lib/core/media.rs), plus the
//! canvas crop that happens before any of that -- see [`cropToSquare`].
//!
//! Upload is three steps, none of which go through `apiFetch`'s JSON body:
//! [`requestUploadUrl`] gets a presigned S3 PUT url, [`uploadToPresignedUrl`]
//! PUTs the cropped blob straight to S3, then [`confirmAvatar`] tells the
//! backend which `picture` column to point at the result -- see the module
//! doc on `core::media` for why nothing is assigned before that last step.

import { apiFetch } from "./api";

// Mirrors `core::media::MAX_AVATAR_SIZE` (backend/lib/core/media.rs): every
// stored avatar/icon is exactly this many pixels square. Cropping to this
// size here is the first of the two safeguards described there -- the
// backend re-squares on confirm regardless of what actually arrives.
export const MAX_AVATAR_SIZE = 256;

const MEDIA_URL = import.meta.env.VITE_MEDIA_URL ?? "";

// The public URL a `picture` key (as returned by `/rbac/me`, a roster
// member, or an org summary) resolves to, or `null` for no picture.
export const avatarUrl = (key) => (key ? `${MEDIA_URL}/${key}` : null);

export const requestUploadUrl = (token) =>
  apiFetch("/media/avatar/upload-url", { method: "POST", token });

// Goes straight to S3 on the presigned URL, not through `apiFetch` --
// different host, binary body, no bearer token or JSON involved.
export const uploadToPresignedUrl = async (uploadUrl, blob) => {
  const response = await fetch(uploadUrl, { method: "PUT", body: blob });
  if (!response.ok) throw new Error(`upload failed (${response.status})`);
};

// `target` is `{ target: "me" }`, `{ target: "org_me", org_id }` or
// `{ target: "org_icon", org_id }` -- mirrors `core::media::Target`. No key
// to name here: the backend derives which staged upload is "yours" from
// the bearer token, not from anything the client says -- see the module
// doc on `core::media` for why that key is never client-supplied.
export const confirmAvatar = (token, target) =>
  apiFetch("/media/avatar/confirm", {
    method: "POST",
    token,
    body: target,
  });

// Clears whichever `picture` column `target` names back to no picture --
// mirrors `confirmAvatar`'s `target` shape, no staged upload involved.
export const removeAvatar = (token, target) =>
  apiFetch("/media/avatar", {
    method: "DELETE",
    token,
    body: target,
  });

// Center-crop `file` to a square (the shorter side) and resample it down to
// `MAX_AVATAR_SIZE` x `MAX_AVATAR_SIZE`, the same normalization the backend
// repeats on confirm. Returns a PNG blob plus the object URL its preview
// `<img>` can use -- the caller owns revoking it.
export function cropToSquare(file) {
  return new Promise((resolve, reject) => {
    const objectUrl = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      const side = Math.min(img.naturalWidth, img.naturalHeight);
      const sx = (img.naturalWidth - side) / 2;
      const sy = (img.naturalHeight - side) / 2;

      const canvas = document.createElement("canvas");
      canvas.width = MAX_AVATAR_SIZE;
      canvas.height = MAX_AVATAR_SIZE;
      canvas
        .getContext("2d")
        .drawImage(
          img,
          sx,
          sy,
          side,
          side,
          0,
          0,
          MAX_AVATAR_SIZE,
          MAX_AVATAR_SIZE,
        );

      URL.revokeObjectURL(objectUrl);
      canvas.toBlob((blob) => {
        if (!blob) {
          reject(new Error("could not encode image"));
          return;
        }
        resolve({ blob, previewUrl: URL.createObjectURL(blob) });
      }, "image/png");
    };
    img.onerror = () => {
      URL.revokeObjectURL(objectUrl);
      reject(new Error("could not read image"));
    };
    img.src = objectUrl;
  });
}
