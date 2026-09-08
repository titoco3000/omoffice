import { wsUrl } from '../api';

/// Channel names, shared with the backend: `chat/mod.rs`, `presence/mod.rs`
/// and `rbac/invites.rs` name the same three.
export const CHANNELS = {
  chat: 'chat',
  invites: 'invites',
  presence: 'presence',
};

/// How often an otherwise-idle socket proves it is still open
const HEARTBEAT_MS = 50_000;

/**
 * One socket for every channel.
 *
 * A feature says which channel it speaks on and never sees the socket: no
 * component opens one, none knows an org's room from an invite toast, and
 * switching orgs does not reconnect anything. The wire frame is
 * `{channel, message}` in both directions -- `core/messenger/mod.rs`'s
 * `envelope`.
 */
export class Messenger {
  #token;
  #socket = null;
  #status = 'closed'; // closed | connecting | open | error
  #handlers = new Map(); // channel -> Set<handler>
  #statusListeners = new Set();
  #queue = []; // frames written before the socket opened
  #context = { org: null, meeting: null };
  #closedByUs = false;
  #heartbeat = null;

  constructor(token) {
    this.#token = token;
  }

  get status() {
    return this.#status;
  }

  connect() {
    if (this.#socket || !this.#token) return;
    this.#closedByUs = false;
    this.#setStatus('connecting');

    const socket = new WebSocket(wsUrl(this.#token));
    this.#socket = socket;

    // Every handler checks that it is still the current socket: a
    // `reconnect` swaps sockets before the old one's `close` event lands,
    // and a stale handler would otherwise drop the new socket on the floor.
    socket.onopen = () => {
      if (this.#socket !== socket) return;
      this.#setStatus('open');
      // The backend starts every connection context-less, so whatever the
      // page already knows about itself is reported first.
      this.#sendContext();
      const queued = this.#queue;
      this.#queue = [];
      for (const frame of queued) socket.send(JSON.stringify(frame));
      this.#heartbeat = setInterval(
        () => this.send({ channel: 'ping', message: {} }),
        HEARTBEAT_MS,
      );
    };
    socket.onerror = () => {
      if (this.#socket !== socket) return;
      this.#setStatus('error');
    };
    socket.onclose = () => {
      if (this.#socket !== socket) return;
      this.#socket = null;
      clearInterval(this.#heartbeat);
      this.#heartbeat = null;
      // A close we asked for is not a failure worth reporting as one.
      this.#setStatus(this.#closedByUs ? 'closed' : 'error');
    };
    socket.onmessage = (event) => {
      if (this.#socket !== socket) return;
      this.#receive(event.data);
    };
  }

  close() {
    this.#closedByUs = true;
    this.#socket?.close();
    this.#socket = null;
    this.#setStatus('closed');
  }

  /// Drop the current socket and open a fresh one -- the UI's "reconnect".
  reconnect() {
    this.#closedByUs = true;
    this.#socket?.close();
    this.#socket = null;
    this.connect();
  }

  /// Send `message` on `channel`. Sent before the socket is open, it waits
  /// in a queue rather than being lost.
  send({ channel, message }) {
    const frame = { channel, message };
    if (this.#socket?.readyState === WebSocket.OPEN) {
      this.#socket.send(JSON.stringify(frame));
    } else {
      this.#queue.push(frame);
    }
  }

  /// Subscribe to `channel`. Returns the unsubscribe function.
  on_message(channel, handler) {
    if (!this.#handlers.has(channel)) this.#handlers.set(channel, new Set());
    this.#handlers.get(channel).add(handler);
    return () => this.#handlers.get(channel)?.delete(handler);
  }

  /// Subscribe to connection status. Returns the unsubscribe function.
  onStatus(listener) {
    this.#statusListeners.add(listener);
    return () => this.#statusListeners.delete(listener);
  }

  /**
   * Report where this tab is: `{org}` from the org switcher, `{meeting}`
   * from a joined meeting. Merged here and sent whole, because the backend
   * stores a context, not a patch (`presence/mod.rs`).
   */
  setContext(patch) {
    const next = { ...this.#context, ...patch };
    if (
      next.org === this.#context.org &&
      next.meeting === this.#context.meeting
    )
      return;
    this.#context = next;
    this.#sendContext();
  }

  #sendContext() {
    this.send({ channel: CHANNELS.presence, message: this.#context });
  }

  #receive(raw) {
    let frame;
    try {
      frame = JSON.parse(raw);
    } catch {
      return; // not JSON -- ignore rather than tear the socket down over one frame
    }
    const handlers = this.#handlers.get(frame?.channel);
    if (!handlers) return;
    for (const handler of handlers) handler(frame.message);
  }

  #setStatus(status) {
    if (status === this.#status) return;
    this.#status = status;
    for (const listener of this.#statusListeners) listener(status);
  }
}
