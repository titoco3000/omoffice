//! Browser side of `core::communication::web_messenger`
//! (backend/lib/core/communication/web_messenger/mod.rs): channels in,
//! channels out, `{channel, message}` frames both ways. A caller talks to
//! `messenger` only -- `connect`/`on`/`send` -- never to a socket; the
//! WebSocket underneath is this module's own business, same as
//! `web_sockets::Hub` is private to the Rust `Messenger` it backs.

import { decodeJwt } from "./jwt";

// `dev`/`prod` talk to `infra/messenger.tf`'s own WebSocket API -- the HTTP
// API `VITE_API_URL` points at cannot proxy a WebSocket handshake at all.
// `local` has no such API (`make run` never gets a WebSocket-API event), so
// there `/ws` is just another route on the one local server `VITE_API_URL`
// already points at.
const WS_URL =
  import.meta.env.VITE_MESSENGER_WS_URL ||
  `${(import.meta.env.VITE_API_URL ?? "").replace(/\/+$/, "")}/ws`.replace(
    /^http/,
    "ws",
  );

// A handshake carries no header to check, so the bearer token rides as
// `?token=...` instead -- see `core::communication::web_messenger::WsAuth`.
function transportUrl(token) {
  return `${WS_URL}?token=${encodeURIComponent(token)}`;
}

class Messenger {
  constructor() {
    this.transport = null;
    this.userId = null;
    this.handlers = new Map();
    // Frames sent before the connection is open, or before `connect` has
    // even been called -- flushed once it is. Lets a component call `send`
    // as soon as it knows what to say, without asking the messenger whether
    // it's ready first.
    this.pending = [];
  }

  /// Open (or, if already open for this user, reuse) the one connection for
  /// `token`. Safe to call from every component that wants the messenger up
  /// -- only the first call for a given user actually connects.
  ///
  /// Closes any stale transport directly rather than through `disconnect`,
  /// which would wipe `pending` -- a component mounted below `AuthProvider`
  /// (e.g. `Panels`' own `org_active` send) can queue a frame before this
  /// ever runs (child effects fire before parent ones), and that frame is
  /// still meant for the connection about to open, not a leftover from one
  /// this call is tearing down.
  connect(token) {
    const userId = decodeJwt(token)?.sub;
    if (!userId || this.userId === userId) return;
    this.transport?.close();
    this.userId = userId;
    const transport = new WebSocket(transportUrl(token));
    transport.onopen = () => {
      this.pending.forEach((frame) => transport.send(frame));
      this.pending = [];
    };
    transport.onmessage = (event) => {
      let frame;
      try {
        frame = JSON.parse(event.data);
      } catch {
        return;
      }
      const handlers = this.handlers.get(frame.channel);
      if (handlers) handlers.forEach((handler) => handler(frame.message));
    };
    this.transport = transport;
  }

  disconnect() {
    this.transport?.close();
    this.transport = null;
    this.userId = null;
    this.pending = [];
  }

  /// Send `message` on `channel`. Queued rather than dropped if the
  /// connection isn't open yet -- see `pending`.
  send(channel, message) {
    const frame = JSON.stringify({ channel, message });
    if (this.transport?.readyState === WebSocket.OPEN) {
      this.transport.send(frame);
    } else {
      this.pending.push(frame);
    }
  }

  /// Register `handler` for `channel`; returns a function that unregisters
  /// it, so a component can wire this straight into a `useEffect` cleanup.
  on(channel, handler) {
    const handlers = this.handlers.get(channel) ?? new Set();
    handlers.add(handler);
    this.handlers.set(channel, handlers);
    return () => handlers.delete(handler);
  }
}

// One instance for the whole tab, same as the backend holds one `Messenger`
// per process -- every component subscribes to this one rather than opening
// a connection of its own.
export const messenger = new Messenger();
