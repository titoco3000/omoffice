//! Modal: pick an image, preview it cropped to the stored square, submit.
//! `target` is `core::media::Target`'s JSON shape -- `{ target: "me" }`,
//! `{ target: "org_me", org_id }` or `{ target: "org_icon", org_id }` -- and
//! decides which `picture` column the upload ends up on; the caller (the
//! one place that knows *why* this panel is open) supplies it and gets
//! `onDone(key)` back once the backend has confirmed and normalized it.

import { useEffect, useState } from "react";
import { useAuth } from "../../core/auth/AuthContext";
import {
  cropToSquare,
  requestUploadUrl,
  uploadToPresignedUrl,
  confirmAvatar,
} from "./api";
import AsyncButton from "../../core/ui/AsyncButton";

export default function AvatarUploadPanel({ title, target, onClose, onDone }) {
  const { token } = useAuth();
  const [picked, setPicked] = useState(null); // { blob, previewUrl }
  const [error, setError] = useState(null);

  // Own the object URL for as long as this panel is open; drop it however
  // the panel closes, not just on a successful submit.
  useEffect(
    () => () => picked && URL.revokeObjectURL(picked.previewUrl),
    [picked],
  );

  const pick = async (event) => {
    const file = event.target.files?.[0];
    if (!file) return;
    setError(null);
    try {
      setPicked(await cropToSquare(file));
    } catch (err) {
      setError(err.message);
    }
  };

  const submit = async () => {
    try {
      const { upload_url } = await requestUploadUrl(token);
      await uploadToPresignedUrl(upload_url, picked.blob);
      // The backend stores the normalized image under a new permanent key,
      // not the staging one just uploaded to -- see core::media's module
      // doc -- so the picture we now show has to come from its response.
      const { key: finalKey } = await confirmAvatar(token, target);
      onDone(finalKey);
      onClose();
    } catch (err) {
      setError(err.message);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="w-full max-w-xs rounded-card border bg-surface p-5 shadow-lg">
        <h2 className="text-lg font-semibold text-ink">{title}</h2>

        <div className="mt-4 flex flex-col items-center gap-3">
          <div className="flex size-40 items-center justify-center overflow-hidden rounded-full border bg-muted">
            {picked ? (
              <img
                src={picked.previewUrl}
                alt=""
                className="size-full object-cover"
              />
            ) : (
              <span className="text-sm text-ink-faint">No image</span>
            )}
          </div>

          <input
            type="file"
            accept="image/*"
            onChange={pick}
            className="text-sm text-ink-muted"
          />
        </div>

        {error && <p className="mt-3 text-sm text-danger">{error}</p>}

        <div className="mt-5 flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded-card px-3 py-1.5 text-sm text-ink-muted hover:bg-muted"
          >
            Cancel
          </button>
          <AsyncButton
            onClick={submit}
            disabled={!picked}
            className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
          >
            Save photo
          </AsyncButton>
        </div>
      </div>
    </div>
  );
}
