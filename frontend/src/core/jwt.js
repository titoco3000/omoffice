//! Reads a JWT's `sub` without verifying it -- verification is the
//! backend's job; this only tells the UI who "me" is among a member list.

export function decodeJwt(token) {
  try {
    const [, payload] = token.split(".");
    return JSON.parse(atob(payload.replace(/-/g, "+").replace(/_/g, "/")));
  } catch {
    return null;
  }
}
