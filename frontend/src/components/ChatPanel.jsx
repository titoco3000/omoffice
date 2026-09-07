import { useEffect, useRef, useState } from "react";
import { CHANNELS, useChannel, useMessenger } from "../messenger/MessengerContext";
import { useOrgContext } from "../org/OrgContext";

/// Caps one tab's kept messages — there is no server history to bound instead.
const MAX_MESSAGES = 200;

/**
 * A room *is* an org: whichever one `OrgSwitcher` (`OrgContext`) has
 * selected. The room the backend fans a message out to is the org this tab
 * reports through `presence` (see `Shell`), so switching orgs needs no
 * socket of its own — this panel only names the `chat` channel.
 */
export default function ChatPanel({ email }) {
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
          {/* Keyed by org id so switching rooms clears the messages of the last one. */}
          <ChatRoom key={selectedOrg.id} email={email} />
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

/** The selected org's room. `email` is only for styling a caller's own messages. */
function ChatRoom({ email }) {
  const { messenger, status } = useMessenger();
  const [messages, setMessages] = useState([]);
  const [draft, setDraft] = useState("");
  const nextIdRef = useRef(0);
  const listRef = useRef(null);

  useChannel(CHANNELS.chat, (message) => {
    if (typeof message?.text !== "string") return;
    nextIdRef.current += 1;
    setMessages((current) => [
      ...current.slice(-(MAX_MESSAGES - 1)),
      { id: nextIdRef.current, from: message.from ?? "?", text: message.text },
    ]);
  });

  useEffect(() => {
    const list = listRef.current;
    if (list) list.scrollTop = list.scrollHeight;
  }, [messages]);

  function send(event) {
    event.preventDefault();
    const text = draft.trim();
    if (!text || status !== "open") return;
    // The backend fans a message out to the sender's own sockets too, so
    // the sender's copy arrives back through `useChannel` like anyone else's.
    messenger.send({ channel: CHANNELS.chat, message: { text } });
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
            onClick={() => messenger?.reconnect()}
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
