// The Makefile exports VITE_API_URL from .env.<stage>; the fallback only
// matters if vite is started by hand instead of through `make run`.
export const API_URL = (
  import.meta.env.VITE_API_URL ?? "http://localhost:8000"
).replace(/\/+$/, "");

export const STAGE = import.meta.env.VITE_STAGE ?? "local";

// Set only on the deployed stages (`infra/make output <stage>`'s
// `chat_ws_url`, from `infra/chat.tf`'s WebSocket API) -- empty locally.
const CHAT_WS_URL = (import.meta.env.VITE_CHAT_WS_URL ?? "").replace(/\/+$/, "");

/// The chat websocket URL for `orgId`. Token rides as a query param either
/// way; `orgId` too when deployed (one fixed route), else as a path segment.
export function chatWsUrl(orgId, token) {
  const query = `token=${encodeURIComponent(token)}&org_id=${encodeURIComponent(orgId)}`;
  if (CHAT_WS_URL) return `${CHAT_WS_URL}?${query}`;
  const base = API_URL.replace(/^http/, "ws");
  return `${base}/chat/ws/${encodeURIComponent(orgId)}?token=${encodeURIComponent(token)}`;
}

/// The room-less presence socket, same deployed-vs-local split as
/// `chatWsUrl` minus the org — delivers an invite toast to a non-member.
export function notifyWsUrl(token) {
  const query = `token=${encodeURIComponent(token)}`;
  if (CHAT_WS_URL) return `${CHAT_WS_URL}?${query}`;
  const base = API_URL.replace(/^http/, "ws");
  return `${base}/chat/ws?${query}`;
}

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

/// Turn a failed `callApi` result into one line, for anything that shows a
/// single error string instead of the whole response.
export function describeFailure({ status, data }) {
  const detail = data?.message ?? data?.error ?? JSON.stringify(data);
  return status === 0 ? `request failed: ${detail}` : `${status}: ${detail}`;
}
