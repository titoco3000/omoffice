//! Browser side of `features::map::realtime`
//! (backend/lib/features/map/realtime.rs): one AppSync Events WebSocket per
//! open map. Moves are delivered by region (`regions.js`), so a socket
//! subscribes only to what is in view -- `setSubscriptions` -- and
//! publishes this user's own moves over the same socket, never touching the
//! API lambda:
//! - fine, `/map/{org_id}/{rx}-{ry}/{user_id}`: while the user's cell is in
//!   that region, at most one per `FINE_PUBLISH_MS`;
//! - coarse, `/map/{org_id}/c/{user_id}`: always, at most one per
//!   `COARSE_PUBLISH_MS`, for viewers zoomed out too far to follow regions.
//! The sender of a received move is whatever the `map` namespace's
//! `onPublish` handler stamped on as `user_id`, taken from the channel path
//! (infra/map_events.tf), never from what a client sent.
//!
//! Only the socket lives here. Fetching the token (`getMapRealtime`), and
//! reconnecting with a fresh one before it expires, is `useMapSession`'s
//! job -- see `openMapEvents`' `onReady`/`onClose`.

import { regionKey, regionOf } from "./regions";

// Published moves per second while moving: 2 fine, 0.2 coarse.
export const FINE_PUBLISH_MS = 500;
export const COARSE_PUBLISH_MS = 5000;

export const coarseChannel = (orgId) => `/map/${orgId}/c/*`;
export const fineChannel = (orgId, key) => `/map/${orgId}/${key}/*`;

const publishChannel = (orgId, key, userId) => `/map/${orgId}/${key}/${userId}`;

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

// At most one call to `send` per `ms`, latest value wins; a value that
// arrives inside the interval is flushed by a timer, so the last one of a
// burst always goes out.
function throttle(ms, send) {
  let last = 0;
  let timer = null;
  let latest = null;
  const flush = () => {
    timer = null;
    last = Date.now();
    const value = latest;
    latest = null;
    send(value);
  };
  return {
    push(value) {
      latest = value;
      if (timer) return;
      const wait = last + ms - Date.now();
      if (wait <= 0) flush();
      else timer = setTimeout(flush, wait);
    },
    cancel() {
      clearTimeout(timer);
      timer = null;
      latest = null;
    },
  };
}

/// Connect to the map's AppSync Events API (`endpoint`, from
/// `GET /map/org/{org_id}/realtime`) as `userId` in `orgId`.
///
/// - `onMove(userId, x, y, facing)` for every move published on a channel
///   subscribed to, this user's own included (the caller filters).
/// - `onReady()` once the whole set last given to `setSubscriptions` is
///   subscribed -- the moment a replacement connection can take over from
///   the one before it.
/// - `onSubscribed()` each time newly added channels finish subscribing,
///   so the caller can fetch what was missed before they were.
/// - `onClose()` once, if the socket goes away without `close()` being
///   called (network drop, AppSync's 24h cap, a missed keep-alive).
///
/// Returns `{setSubscriptions(channels), publish(x, y, facing), close()}`.
/// Nothing is subscribed until `setSubscriptions`; the latest set is kept
/// and subscribed on `connection_ack` if it was given before then. A
/// `publish` before the socket is open is held and sent on open -- only the
/// latest, since an older cell is no use to anyone once a newer one exists.
export function openMapEvents({
  endpoint,
  token,
  orgId,
  userId,
  onMove,
  onReady,
  onSubscribed,
  onClose,
}) {
  const { http, realtime } = hosts(endpoint);
  const authorization = { Authorization: token, host: http };
  const socket = new WebSocket(`wss://${realtime}/event/realtime`, [
    "aws-appsync-event-ws",
    `header-${base64Url(authorization)}`,
  ]);
  let open = false;
  let closed = false;
  let heldMove = null;
  let keepAliveTimer = null;
  let keepAliveMs = 300000;
  // Subscriptions: the set wanted, what has been sent (id -> channel), and
  // which of those the server has yet to confirm.
  let desired = null; // Set of channels; null until `setSubscriptions`
  const subscribed = new Map(); // channel -> id
  const channelOf = new Map(); // id -> channel
  const pending = new Set(); // ids
  let ready = false;
  // Last time a fine-channel move of each user arrived -- a coarse one
  // that lands soon after is older than it, and is dropped.
  const lastFineAt = new Map();
  // Region the last move was published from.
  let lastRegion = null;

  const send = (frame) => socket.send(JSON.stringify(frame));
  const sendTo = (key, { x, y, facing }) => {
    if (!open || socket.readyState !== WebSocket.OPEN) return;
    send({
      type: "publish",
      id: crypto.randomUUID(),
      channel: publishChannel(orgId, key, userId),
      events: [JSON.stringify({ x, y, facing })],
      authorization,
    });
  };
  const fine = throttle(FINE_PUBLISH_MS, (move) => sendTo(move.region, move));
  const coarse = throttle(COARSE_PUBLISH_MS, (move) => sendTo("c", move));

  const syncSubscriptions = () => {
    if (!open || !desired) return;
    let added = false;
    for (const [channel, id] of [...subscribed]) {
      if (desired.has(channel)) continue;
      send({ type: "unsubscribe", id });
      subscribed.delete(channel);
      channelOf.delete(id);
      pending.delete(id);
    }
    for (const channel of desired) {
      if (subscribed.has(channel)) continue;
      const id = crypto.randomUUID();
      subscribed.set(channel, id);
      channelOf.set(id, channel);
      pending.add(id);
      added = true;
      send({ type: "subscribe", id, channel, authorization });
    }
    if (!added && pending.size === 0) settle(false);
  };
  // The wanted set is fully subscribed.
  const settle = (added) => {
    if (pending.size > 0) return;
    if (added) onSubscribed?.();
    if (!ready) {
      ready = true;
      onReady?.();
    }
  };

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
    fine.cancel();
    coarse.cancel();
    socket.close();
  };

  const publish = (x, y, facing) => {
    if (!open) {
      heldMove = { x, y, facing };
      return;
    }
    const [rx, ry] = regionOf(x, y);
    const region = regionKey(rx, ry);
    const move = { x, y, facing, region };
    // Viewers still on the region just left would otherwise keep the last
    // cell they saw there -- tell them once where this user went.
    if (lastRegion && lastRegion !== region) sendTo(lastRegion, move);
    lastRegion = region;
    fine.push(move);
    coarse.push(move);
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
        open = true;
        if (heldMove) publish(heldMove.x, heldMove.y, heldMove.facing);
        heldMove = null;
        syncSubscriptions();
        break;
      case "ka":
        armKeepAlive();
        break;
      case "subscribe_success":
        if (pending.delete(frame.id)) settle(true);
        break;
      case "data": {
        const channel = channelOf.get(frame.id);
        if (!channel) break;
        const isCoarse = channel === coarseChannel(orgId);
        for (const move of parseEvents(frame.event)) {
          if (
            !move?.user_id ||
            !Number.isFinite(move.x) ||
            !Number.isFinite(move.y)
          )
            continue;
          if (isCoarse) {
            const fineAt = lastFineAt.get(move.user_id) ?? 0;
            if (Date.now() - fineAt < COARSE_PUBLISH_MS * 2) continue;
          } else lastFineAt.set(move.user_id, Date.now());
          onMove(move.user_id, move.x, move.y, move.facing);
        }
        break;
      }
      case "subscribe_error":
      case "connection_error":
      case "error":
        console.warn("map realtime:", frame.type, frame.errors ?? frame);
        socket.close();
        break;
      case "publish_error":
        console.warn("map realtime: publish_error", frame.errors ?? frame);
        break;
      default:
        // publish_success: nothing to do. A lost move is superseded by the
        // next one, or by the stored position on the next load.
        break;
    }
  };
  socket.onclose = () => {
    clearTimeout(keepAliveTimer);
    fine.cancel();
    coarse.cancel();
    if (closed) return;
    closed = true;
    onClose?.();
  };

  return {
    setSubscriptions(channels) {
      desired = new Set(channels);
      syncSubscriptions();
    },
    publish,
    close,
  };
}

/// Publish one move as `userId` in `orgId` over AppSync's HTTP endpoint,
/// without opening (or subscribing on) a socket -- for a tab that isn't
/// showing the map, and so has no connection, but still has to tell those
/// that are where it went (`MapRelocationProvider`). Same channels, and so
/// the same authorizer check, as `openMapEvents`' own publish: the fine one
/// of the cell's region, and the coarse one.
export async function publishMapMove({
  endpoint,
  token,
  orgId,
  userId,
  x,
  y,
  facing,
}) {
  const { http } = hosts(endpoint);
  const [rx, ry] = regionOf(x, y);
  await Promise.all(
    [regionKey(rx, ry), "c"].map(async (key) => {
      const response = await fetch(`https://${http}/event`, {
        method: "POST",
        headers: { "content-type": "application/json", authorization: token },
        body: JSON.stringify({
          channel: publishChannel(orgId, key, userId),
          events: [JSON.stringify({ x, y, facing })],
        }),
      });
      if (!response.ok) throw new Error(`publish failed: ${response.status}`);
    }),
  );
}
