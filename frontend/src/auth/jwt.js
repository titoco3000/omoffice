// Decoding a JWT payload client-side needs no library: it's a base64url JSON
// blob between two dots. This never verifies a signature — the backend
// already did that — it only reads `sub`/`exp` so the UI knows who's logged
// in and when to stop treating a stored token as valid.

/// The `{sub, iat, exp}` claims from a token, or `null` if it isn't shaped
/// like one. Never throws.
export function decodeClaims(token) {
  if (!token) return null;
  const parts = token.split(".");
  if (parts.length !== 3) return null;
  try {
    const payload = parts[1].replace(/-/g, "+").replace(/_/g, "/");
    const padded = payload.padEnd(payload.length + ((4 - (payload.length % 4)) % 4), "=");
    return JSON.parse(atob(padded));
  } catch {
    return null;
  }
}

/// True if `claims.exp` (unix seconds) is in the past, or `claims` is missing
/// one.
export function isExpired(claims) {
  if (!claims?.exp) return true;
  return claims.exp * 1000 <= Date.now();
}
