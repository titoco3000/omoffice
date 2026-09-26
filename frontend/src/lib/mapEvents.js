//! Browser side of `features::map::realtime`
//! (backend/lib/features/map/realtime.rs): one AppSync Events WebSocket per
//! open map, subscribed to `/map/{org_id}/*` and publishing this user's own
//! moves to `/map/{org_id}/{user_id}` over the same socket -- live moves
//! never touch the API lambda. The sender of a received move is whatever
//! the `map` namespace's `onPublish` handler stamped on as `user_id`, taken
//! from the channel path (infra/map_events.tf), never from what a client
//! sent.
//!
//! Only the socket lives here. Fetching the token (`getMapRealtime`), and
//! reconnecting with a fresh one before it expires, is `useMapSession`'s
//! job -- see `openMapEvents`' `onReady`/`onClose`.

// The realtime host sits next to the HTTP one AppSync gives the API:
// `{id}.appsync-api.{region}...` -> `{id}.appsync-realtime-api.{region}...`.
function hosts(endpoint) {
  const http = endpoint.replace(/^\w+:\/\//, "").replace(/\/.*$/, "");
  return {
    http,
    realtime: http.replace(".appsync-api.", ".appsync-realtime-api."),
  };
}

// AppSync wants the auth header as a base64url subprotocol on the handshake.
function base64Url(value) {
  return btoa(JSON.stringify(value))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

// Data frames document `event` as a JSON string; tolerate an array of them.
function parseEvents(event) {
  const raw = Array.isArray(event) ? event : [event];
  return raw.flatMap((item) => {
    try {
      return [typeof item === "string" ? JSON.parse(item) : item];
    } catch {
      return [];
    }
  });
}

/// Connect to the map's AppSync Events API (`endpoint`, from
/// `GET /map/org/{org_id}/realtime`) as `userId` in `orgId`.
///
/// - `onMove(userId, x, y)` for every move published in the org, this
///   user's own included (the caller filters).
/// - `onReady()` once subscribed -- the moment a replacement connection can
///   take over from the one before it.
/// - `onClose()` once, if the socket goes away without `close()` being
///   called (network drop, AppSync's 24h cap, a missed keep-alive).
///
/// Returns `{publish(x, y), close()}`. A `publish` before the socket is
/// open is held and sent on open -- only the latest, since an older cell
/// is no use to anyone once a newer one exists.
export function openMapEvents({
  endpoint,
  token,
  orgId,
  userId,
  onMove,
  onReady,
  onClose,
}) {
  const { http, realtime } = hosts(endpoint);
  const authorization = { Authorization: token, host: http };
  const socket = new WebSocket(`wss://${realtime}/event/realtime`, [
    "aws-appsync-event-ws",
    `header-${base64Url(authorization)}`,
  ]);
  const subscriptionId = crypto.randomUUID();
  let open = false;
  let closed = false;
  let heldMove = null;
  let keepAliveTimer = null;
  let keepAliveMs = 300000;

  const send = (frame) => socket.send(JSON.stringify(frame));
  const sendMove = ({ x, y }) =>
    send({
      type: "publish",
      id: crypto.randomUUID(),
      channel: `/map/${orgId}/${userId}`,
      events: [JSON.stringify({ x, y })],
      authorization,
    });

  // No keep-alive within `connectionTimeoutMs` means the connection is
  // dead without having said so.
  const armKeepAlive = () => {
    clearTimeout(keepAliveTimer);
    keepAliveTimer = setTimeout(() => socket.close(), keepAliveMs);
  };

  const close = () => {
    if (closed) return;
    closed = true;
    clearTimeout(keepAliveTimer);
    socket.close();
  };

  socket.onopen = () => send({ type: "connection_init" });
  socket.onmessage = (message) => {
    let frame;
    try {
      frame = JSON.parse(message.data);
    } catch {
      return;
    }
    switch (frame.type) {
      case "connection_ack":
        keepAliveMs = frame.connectionTimeoutMs ?? keepAliveMs;
        armKeepAlive();
        send({
          type: "subscribe",
          id: subscriptionId,
          channel: `/map/${orgId}/*`,
          authorization,
        });
        break;
      case "ka":
        armKeepAlive();
        break;
      case "subscribe_success":
        open = true;
        if (heldMove) sendMove(heldMove);
        heldMove = null;
        onReady?.();
        break;
      case "data":
        if (frame.id !== subscriptionId) break;
        for (const move of parseEvents(frame.event)) {
          if (
            move?.user_id &&
            Number.isFinite(move.x) &&
            Number.isFinite(move.y)
          )
            onMove(move.user_id, move.x, move.y);
        }
        break;
      case "subscribe_error":
      case "connection_error":
      case "error":
        socket.close();
        break;
      default:
        // publish_success/publish_error: a lost move is superseded by the
        // next one, or by the stored position on the next load.
        break;
    }
  };
  socket.onclose = () => {
    clearTimeout(keepAliveTimer);
    if (closed) return;
    closed = true;
    onClose?.();
  };

  return {
    publish(x, y) {
      if (open && socket.readyState === WebSocket.OPEN) sendMove({ x, y });
      else heldMove = { x, y };
    },
    close,
  };
}
