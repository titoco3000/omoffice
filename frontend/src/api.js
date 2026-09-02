// The Makefile exports VITE_API_URL from .env.<stage>; the fallback only
// matters if vite is started by hand instead of through `make run`.
export const API_URL = (
  import.meta.env.VITE_API_URL ?? "http://localhost:8000"
).replace(/\/+$/, "");

export const STAGE = import.meta.env.VITE_STAGE ?? "local";

/// Placeholders in a route template, e.g. "/private/sample-post/{var}" -> ["var"].
export function pathParams(route) {
  return [...route.matchAll(/\{(\w+)\}/g)].map((match) => match[1]);
}

/// Fill a route template with `values`, percent-encoding each one.
export function buildPath(route, values) {
  return route.replace(/\{(\w+)\}/g, (_, name) =>
    encodeURIComponent(values[name] ?? ""),
  );
}

/// Call the API and always resolve to the same shape, so the caller renders
/// a failed request the same way it renders a 401.
export async function callApi({ method = "GET", path, token, body }) {
  const headers = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";

  const started = performance.now();
  let response;
  try {
    response = await fetch(`${API_URL}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (error) {
    // A CORS rejection and an unreachable server both land here.
    return {
      ok: false,
      status: 0,
      ms: Math.round(performance.now() - started),
      data: { error: "network_error", message: String(error) },
    };
  }

  const text = await response.text();
  let data;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = text;
  }

  return {
    ok: response.ok,
    status: response.status,
    ms: Math.round(performance.now() - started),
    data,
  };
}
