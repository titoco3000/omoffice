import { useEffect, useRef, useState } from "react";
import { chatWsUrl } from "../api";
import { useOrgContext } from "../org/OrgContext";

/// A room holds at most this many messages in one tab -- there is no
/// history (`backend/src/chat/mod.rs`: nothing is written to a table), so
/// this only bounds how much one already-open tab keeps around, not what a
/// new join can see.
const MAX_MESSAGES = 200;

/**
 * A room *is* an org, every member shares the one the backend keeps for it
 * -- so which room to join is just whichever org `OrgSwitcher` currently has
 * selected (`OrgContext`), not a picker of its own the way this used to
 * have. Switching orgs from the header closes this room and opens the new
 * one, same as picking a different org here used to.
 */
export default function ChatPanel({ token, email }) {
  const { orgs, loading, error, selectedOrg } = useOrgContext();

  if (selectedOrg) {
    return (
      <section className="rounded-xl border border-slate-800 bg-slate-900/60 p-4">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-medium text-slate-300">chat</h2>
          <span className="rounded bg-emerald-500/15 px-2 py-0.5 text-xs text-emerald-300">
            live
          </span>
          <span className="ml-auto min-w-0 truncate text-xs text-slate-500">
            {selectedOrg.name}
          </span>
        </div>
        <div className="mt-3">
          {/* Keyed by org id so switching rooms opens a fresh socket. */}
          <ChatRoom key={selectedOrg.id} orgId={selectedOrg.id} token={token} email={email} />
        </div>
      </section>
    );
  }

  return (
    <section className="rounded-xl border border-slate-800 bg-slate-900/60 p-4">
      <h2 className="text-sm font-medium text-slate-300">chat</h2>

      {error && (
        <p className="mt-3 font-mono text-xs break-all text-rose-300">{error}</p>
      )}
      {!error && orgs.length === 0 && (
        <p className="mt-3 text-xs text-slate-600">
          {loading
            ? "loading..."
            : 'not a member of any org yet -- join or create one under "organizations"'}
        </p>
      )}
    </section>
  );
}

/**
 * One org's room: opens the socket on mount, closes it on unmount, and
 * fans every `{"text": ...}` frame it sends into what every other member
 * currently connected receives -- see `chat/mod.rs` for both halves of that
 * (a real held-open socket locally, API Gateway's WebSocket API deployed).
 * `email` is only for styling a caller's own messages differently; the
 * server never needs to be told who's talking, it already knows from the
 * connection.
 */
function ChatRoom({ orgId, token, email }) {
  const [status, setStatus] = useState("connecting"); // connecting | open | closed | error
  const [messages, setMessages] = useState([]);
  const [draft, setDraft] = useState("");
  const [attempt, setAttempt] = useState(0); // bump to force a fresh socket
  const socketRef = useRef(null);
  const nextIdRef = useRef(0);
  const listRef = useRef(null);

  useEffect(() => {
    const socket = new WebSocket(chatWsUrl(orgId, token));
    socketRef.current = socket;

    socket.onopen = () => setStatus("open");
    socket.onerror = () => setStatus("error");
    socket.onclose = () => setStatus("closed");
    socket.onmessage = (event) => {
      let parsed;
      try {
        parsed = JSON.parse(event.data);
      } catch {
        return; // not JSON -- ignore rather than crash the room over one frame
      }
      if (typeof parsed?.text !== "string") return;

      nextIdRef.current += 1;
      setMessages((current) => [
        ...current.slice(-(MAX_MESSAGES - 1)),
        { id: nextIdRef.current, from: parsed.from ?? "?", text: parsed.text },
      ]);
    };

    return () => socket.close();
  }, [orgId, token, attempt]);

  useEffect(() => {
    const list = listRef.current;
    if (list) list.scrollTop = list.scrollHeight;
  }, [messages]);

  function send(event) {
    event.preventDefault();
    const text = draft.trim();
    if (!text || status !== "open") return;
    socketRef.current?.send(JSON.stringify({ text }));

    // The backend never echoes a message back to whoever sent it (both
    // `chat::ws` and `chat::lambda_entry` fan out to every *other* member
    // on purpose) -- so the sender's own copy is added here, not read back
    // off the socket.
    nextIdRef.current += 1;
    setMessages((current) => [
      ...current.slice(-(MAX_MESSAGES - 1)),
      { id: nextIdRef.current, from: email, text },
    ]);
    setDraft("");
  }

  return (
    <div className="grid gap-3">
      <div ref={listRef} className="h-64 overflow-y-auto rounded-lg bg-slate-950 p-3">
        {messages.length === 0 && (
          <p className="text-xs text-slate-600">
            {status === "open" ? "no messages yet" : status}
          </p>
        )}
        <ul className="grid gap-1">
          {messages.map((message) => {
            const isSelf = message.from === email;
            return (
              <li
                key={message.id}
                className={`flex ${isSelf ? "justify-end" : "justify-start"}`}
              >
                <div
                  className={`max-w-[80%] rounded-lg px-3 py-1.5 text-sm break-words ${
                    isSelf
                      ? "bg-sky-600 text-white"
                      : "bg-slate-800 text-slate-200"
                  }`}
                >
                  {!isSelf && (
                    <div className="font-mono text-xs text-slate-400">{message.from}</div>
                  )}
                  {message.text}
                </div>
              </li>
            );
          })}
        </ul>
      </div>

      <form onSubmit={send} className="flex gap-2">
        <input
          type="text"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={status === "open" ? "message" : status}
          disabled={status !== "open"}
          className="h-9 min-w-0 flex-1 rounded-lg border border-slate-700 bg-slate-950 px-2 text-sm text-slate-100 outline-none focus:border-sky-500 disabled:opacity-50"
        />
        {(status === "closed" || status === "error") && (
          <button
            type="button"
            onClick={() => {
              // Immediate feedback -- the new socket's own `onopen` will
              // confirm it a moment later; messages are left as they are,
              // a reconnect resumes the same room rather than clearing it.
              setStatus("connecting");
              setAttempt((count) => count + 1);
            }}
            className="shrink-0 rounded-lg bg-slate-700 px-4 py-1.5 text-sm font-medium text-white hover:bg-slate-600"
          >
            reconnect
          </button>
        )}
        <button
          type="submit"
          disabled={status !== "open" || !draft.trim()}
          className="shrink-0 rounded-lg bg-sky-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
        >
          send
        </button>
      </form>
    </div>
  );
}