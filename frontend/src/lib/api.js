//! Thin fetch wrapper: builds on `VITE_API_URL`, attaches the bearer token,
//! and turns a non-2xx response into an [`ApiError`] carrying the backend's
//! own `{error, message}` body (see `core::data::db::DbError`'s
//! `IntoResponse` and its siblings on the Rust side).
//!
//! A 401 means `require_bearer` rejected the token outright (missing,
//! malformed, expired) -- not "this action needs a different role", which
//! is a 403 and left for the caller to handle. `AuthContext` listens for
//! [`UNAUTHORIZED_EVENT`] to clear the dead token and bounce to `/login`,
//! instead of every panel quietly re-firing the same doomed request forever.

const BASE_URL = import.meta.env.VITE_API_URL ?? "";

export const UNAUTHORIZED_EVENT = "omoffice:unauthorized";

export class ApiError extends Error {
  constructor(status, body) {
    super(body?.message ?? `request failed (${status})`);
    this.status = status;
    this.code = body?.error;
  }
}

export async function apiFetch(path, { method = "GET", token, body } = {}) {
  const response = await fetch(`${BASE_URL}${path}`, {
    method,
    headers: {
      ...(body ? { "Content-Type": "application/json" } : {}),
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });

  if (response.status === 401 && token) {
    window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
  }

  const data = response.status === 204 ? null : await response.json().catch(() => null);
  if (!response.ok) throw new ApiError(response.status, data);
  return data;
}
