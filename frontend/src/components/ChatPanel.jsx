import { useEffect, useRef, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { CHANNELS, useChannel, useMessenger } from "../messenger/MessengerContext";
import { useOrgContext } from "../org/OrgContext";
import Button from "../ui/Button";
import Input from "../ui/Input";
import { IconClose } from "../ui/icons";

/// Caps one tab's kept messages — there is no server history to bound instead.
const MAX_MESSAGES = 200;

/**
 * A room *is* an org: whichever one is selected. The room the backend fans a
 * message out to is the org this tab reports through `presence`, so
 * switching orgs needs no socket of its own — this panel only names the
 * `chat` channel.
 *
 * It fills its column, so the caller decides whether that column is the
 * meeting's second sidebar or a sheet over the video.
 */
export default function ChatPanel({ onClose }) {
  const { selectedOrg, loading } = useOrgContext();

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2.5">
        <h2 className="min-w-0 flex-1 truncate text-sm font-semibold text-ink">
          chat
        </h2>
        <span className="min-w-0 truncate text-xs text-ink-faint">
          {selectedOrg?.name}
        </span>
        {onClose && (
          <Button variant="ghost" size="icon-sm" aria-label="close chat" onClick={onClose}>
            <IconClose className="h-4 w-4" />
          </Button>
        )}
      </div>

      {selectedOrg ? (
        /* Keyed by org id so switching rooms clears the last one's messages. */
        <ChatRoom key={selectedOrg.id} />
      ) : (
        <p className="p-3 text-xs text-ink-faint">
          {loading ? "loading..." : "no org selected"}
        </p>
      )}
    </div>
  );
}

/** The selected org's room. Own messages are styled by the caller's email. */
function ChatRoom() {
  const { email } = useAuth();
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
    <>
      <div ref={listRef} className="min-h-0 flex-1 overflow-y-auto p-3">
        {messages.length === 0 && (
          <p className="text-xs text-ink-faint">
            {status === "open" ? "no messages yet" : status}
          </p>
        )}
        <ul className="grid gap-1.5">
          {messages.map((message) => {
            const isSelf = message.from === email;
            return (
              <li
                key={message.id}
                className={`flex ${isSelf ? "justify-end" : "justify-start"}`}
              >
                <div
                  className={`max-w-[85%] rounded-lg px-3 py-1.5 text-sm break-words ${
                    isSelf ? "bg-brand text-on-brand" : "bg-muted text-ink"
                  }`}
                >
                  {!isSelf && (
                    <div className="font-mono text-xs opacity-70">{message.from}</div>
                  )}
                  {message.text}
                </div>
              </li>
            );
          })}
        </ul>
      </div>

      <form onSubmit={send} className="flex shrink-0 gap-2 border-t border-line p-2">
        <Input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={status === "open" ? "message" : status}
          disabled={status !== "open"}
          className="flex-1"
        />
        {(status === "closed" || status === "error") && (
          <Button variant="secondary" onClick={() => messenger?.reconnect()}>
            reconnect
          </Button>
        )}
        <Button type="submit" disabled={status !== "open" || !draft.trim()}>
          send
        </Button>
      </form>
    </>
  );
}
