import { useState } from "react";
import { preferredName } from "../lib/names";

export default function MeetingChat({
  messages,
  members,
  onSend,
  className = "",
}) {
  const [draft, setDraft] = useState("");

  const submit = (event) => {
    event.preventDefault();
    if (!draft.trim()) return;
    onSend(draft);
    setDraft("");
  };

  return (
    <div className={`flex min-h-0 flex-col ${className}`}>
      <ul className="min-h-0 flex-1 space-y-1 overflow-y-auto p-2 text-sm">
        {messages.length === 0 && (
          <li className="text-ink-muted">No messages yet.</li>
        )}
        {messages.map((msg, index) => (
          <li key={index}>
            <span className="text-ink-muted">
              {msg.self ? "You" : preferredName(msg.from, members)}:
            </span>{" "}
            <span className="text-ink">{msg.text}</span>
          </li>
        ))}
      </ul>
      <form onSubmit={submit} className="flex shrink-0 gap-1 border-t p-2">
        <input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="Message everyone"
          className="min-w-0 flex-1 rounded-card border bg-bg px-2 py-1 text-sm text-ink"
        />
        <button
          type="submit"
          disabled={!draft.trim()}
          className="rounded-card border px-2 py-1 text-sm text-brand hover:underline disabled:opacity-50"
        >
          Send
        </button>
      </form>
    </div>
  );
}
