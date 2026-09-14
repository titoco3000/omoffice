import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { decodeJwt } from "../lib/jwt";
import { preferredName } from "../lib/names";
import * as chime from "../lib/chime";

// The eight places a mini-player may rest are the 3x3 grid from
// dev-docs/phase/6.md with its center square dropped (that's where the page
// content lives) -- see `nearestSlot` and `slotPosition` below.
const SNAP_MARGIN = 16; // px between the panel and the viewport edge when docked

// Given the panel's own size, where a named slot puts its top-left corner.
// Kept in plain top/left pixels (never right/bottom, never a translate) so
// every slot animates through the same two CSS properties.
function slotPosition(slot, size) {
  const [row, col] = slot.split("-");
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const left =
    col === "left"
      ? SNAP_MARGIN
      : col === "right"
        ? vw - size.width - SNAP_MARGIN
        : (vw - size.width) / 2;
  const top =
    row === "top"
      ? SNAP_MARGIN
      : row === "bot"
        ? vh - size.height - SNAP_MARGIN
        : (vh - size.height) / 2;
  return { top, left };
}

// Which slot a dropped panel center is closest to. The viewport is divided
// into thirds each way; "mid-center" isn't a real slot, so a drop there
// falls back to whichever side of the screen it's nearer to.
function nearestSlot(centerX, centerY) {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const col = centerX < vw / 3 ? "left" : centerX < (2 * vw) / 3 ? "center" : "right";
  const row = centerY < vh / 3 ? "top" : centerY < (2 * vh) / 3 ? "mid" : "bot";
  if (row === "mid" && col === "center") {
    return centerX < vw / 2 ? "mid-left" : "mid-right";
  }
  return `${row}-${col}`;
}

export default function MeetingsPanel({ token, session, members }) {
  const myId = decodeJwt(token)?.sub;
  const [chatDraft, setChatDraft] = useState("");
  const [mode, setMode] = useState("mini"); // "mini" | "full"
  const [chatOpen, setChatOpen] = useState(true);
  const [position, setPosition] = useState("bot-right"); // last dropped slot, mini mode only
  const [drag, setDrag] = useState(null); // {pointerId, grabX, grabY, x, y} while dragging
  const [size, setSize] = useState({ width: 0, height: 0 }); // measured panel size

  const panelRef = useRef(null);

  const {
    audioRef,
    selfVideoRef,
    meetings,
    myMeetingId,
    roomStatus,
    cameraOn,
    micMuted,
    sharingScreen,
    remoteTiles,
    error,
    chatMessages,
    sendChatMessage,
    muteAttendee,
    turnOffAttendeeCamera,
    leave,
    toggleCamera,
    toggleMic,
    toggleScreenShare,
  } = session;

  // Track the panel's own footprint -- it changes with attendee count, chat
  // visibility, and mode -- so a mini-player docked in a corner keeps hugging
  // that corner instead of drifting as it grows or shrinks.
  //
  // Keyed on `myMeetingId`, not `[]`: the `<section>` below doesn't exist
  // until a meeting starts (see the early `return null`), so `panelRef` is
  // still null on this hook's very first, meeting-less mount. Re-running
  // when `myMeetingId` changes is what catches the ref once it's actually
  // attached -- same reason `useMeetingSession`'s `selfVideoRef` effect is
  // keyed on it too.
  useLayoutEffect(() => {
    const element = panelRef.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      const box = entry.contentBoxSize?.[0];
      setSize(
        box
          ? { width: box.inlineSize, height: box.blockSize }
          : { width: entry.contentRect.width, height: entry.contentRect.height },
      );
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [myMeetingId]);

  // The viewport itself can change (window resize) independently of the
  // panel's own size -- re-anchor to the same named slot when it does.
  const [, forceRecompute] = useState(0);
  useEffect(() => {
    const onResize = () => forceRecompute((n) => n + 1);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const startDrag = (event) => {
    if (mode !== "mini") return;
    const rect = panelRef.current.getBoundingClientRect();
    event.currentTarget.setPointerCapture(event.pointerId);
    setDrag({
      pointerId: event.pointerId,
      grabX: event.clientX - rect.left,
      grabY: event.clientY - rect.top,
      x: rect.left,
      y: rect.top,
    });
  };

  const dragMove = (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    setDrag((d) => ({
      ...d,
      x: event.clientX - d.grabX,
      y: event.clientY - d.grabY,
    }));
  };

  const dragEnd = (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    setPosition(nearestSlot(drag.x + size.width / 2, drag.y + size.height / 2));
    setDrag(null);
  };

  const dockedStyle =
    mode === "full"
      ? undefined
      : drag
        ? { top: drag.y, left: drag.x }
        : { ...slotPosition(position, size) };

  const submitChat = (event) => {
    event.preventDefault();
    sendChatMessage(chatDraft);
    setChatDraft("");
  };

  // Everyone else currently in this tab's own meeting -- from the meeting
  // list's own attendee roster (`meeting.attendees`), not `remoteTiles`:
  // this has to work for muting/camera-off even before Chime has a video
  // tile for them (audio-only, or still "waiting").
  const otherAttendees = (
    meetings.find((m) => m.id === myMeetingId)?.attendees ?? []
  ).filter((userId) => userId !== myId);

  // The dock is the active-meeting surface only -- nothing to float over the
  // page when this tab isn't in one. Starting/joining a meeting lives in
  // `MeetingsListPanel` instead, which stays in the normal page flow.
  if (!myMeetingId) return null;

  return (
    <section
      ref={panelRef}
      style={dockedStyle}
      className={
        mode === "full"
          ? "fixed inset-4 z-50 flex flex-col rounded-card border bg-surface shadow-xl"
          : `fixed z-50 flex max-w-sm flex-col rounded-card border bg-surface shadow-xl ${
              drag ? "" : "transition-[top,left] duration-200"
            }`
      }
    >
      {/* Chime's audio output -- never rendered as a control, just a sink. */}
      <audio ref={audioRef} autoPlay hidden />

      {/* Drag handle in mini mode; inert in full mode. Buttons stop the
          pointerdown from bubbling here so clicking them doesn't start a
          drag. */}
      <header
        onPointerDown={startDrag}
        onPointerMove={dragMove}
        onPointerUp={dragEnd}
        className={`flex shrink-0 items-center justify-between rounded-t-card border-b bg-bg px-3 py-2 ${
          mode === "mini" ? "cursor-move touch-none" : ""
        }`}
      >
        <h2 className="text-sm font-semibold text-ink">Meetings</h2>
        <div
          className="flex items-center gap-2"
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button
            onClick={() => setChatOpen((v) => !v)}
            title={chatOpen ? "Hide chat" : "Show chat"}
            className="rounded-card border px-2 py-1 text-xs text-ink-muted hover:text-ink"
          >
            Chat
          </button>
          <button
            onClick={() => setMode((m) => (m === "full" ? "mini" : "full"))}
            title={mode === "full" ? "Switch to mini-player" : "Switch to full screen"}
            className="rounded-card border px-2 py-1 text-xs text-ink-muted hover:text-ink"
          >
            {mode === "full" ? "Minimize" : "Full screen"}
          </button>
          <button onClick={leave} className="text-xs text-danger hover:underline">
            Leave
          </button>
        </div>
      </header>

      <div className="flex min-h-0 flex-1">
        <div
          className={`min-w-0 flex-1 overflow-y-auto p-3 ${
            mode === "mini" ? "max-h-[70vh]" : ""
          }`}
        >
          <div className="rounded-card border bg-bg p-3">
            <div className="flex flex-wrap gap-2">
              {/* Own preview -- a chimeless meeting still shows this, see
                  `setCameraOn` in lib/chime.js. Mirrored and muted: it's a
                  mirror of yourself, not a source of sound. */}
              <video
                ref={selfVideoRef}
                autoPlay
                muted
                playsInline
                className="aspect-video w-40 -scale-x-100 rounded-card bg-black object-cover"
              />
              {remoteTiles.map((tile) => (
                <video
                  key={tile.tileId}
                  ref={(element) => chime.bindRemoteVideo(tile.tileId, element)}
                  autoPlay
                  playsInline
                  className={`aspect-video rounded-card bg-black object-cover ${
                    tile.isContent ? "w-full sm:w-80" : "w-40"
                  }`}
                />
              ))}
            </div>

            <div className="mt-3 flex flex-wrap gap-2 text-sm">
              <button
                onClick={toggleCamera}
                className={`rounded-card border px-3 py-1.5 ${cameraOn ? "text-ink" : "text-ink-muted"}`}
              >
                {cameraOn ? "Turn camera off" : "Turn camera on"}
              </button>
              <button
                onClick={toggleMic}
                className={`rounded-card border px-3 py-1.5 ${micMuted ? "text-ink-muted" : "text-ink"}`}
              >
                {micMuted ? "Unmute" : "Mute"}
              </button>
              <button
                onClick={toggleScreenShare}
                disabled={roomStatus !== "connected"}
                className={`rounded-card border px-3 py-1.5 disabled:opacity-50 ${sharingScreen ? "text-ink" : "text-ink-muted"}`}
              >
                {sharingScreen ? "Stop sharing screen" : "Share screen"}
              </button>
            </div>

            <p className="mt-3 text-xs text-ink-muted">
              {roomStatus === "connected" ? "Connected" : "Waiting for others"}
            </p>

            {/* Any attendee may force any other attendee's mic or camera off
                -- see `features::meetings::controls`
                (backend/lib/features/meetings/controls.rs). No accept step,
                and no way to force either back *on* -- only the target's own
                tab can do that. */}
            {otherAttendees.length > 0 && (
              <ul className="mt-3 space-y-1 text-sm">
                {otherAttendees.map((userId) => (
                  <li
                    key={userId}
                    className="flex items-center justify-between rounded-card border px-2 py-1"
                  >
                    <span className="text-ink">
                      {preferredName(userId, members)}
                    </span>
                    <span className="flex gap-2">
                      <button
                        onClick={() => muteAttendee(userId)}
                        className="text-ink-muted hover:text-ink"
                      >
                        Mute
                      </button>
                      <button
                        onClick={() => turnOffAttendeeCamera(userId)}
                        className="text-ink-muted hover:text-ink"
                      >
                        Turn off camera
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>

          {error && <p className="mt-3 text-sm text-danger">{error}</p>}
        </div>

        {/* No history behind this -- see `useMeetingSession`'s
            `chatMessages`: nothing is stored server-side, only what this tab
            has sent or received since joining. Its own column so it can be
            hidden without touching the video/controls layout next to it. */}
        {chatOpen && (
          <div
            className={`flex shrink-0 flex-col border-l bg-surface ${
              mode === "full" ? "w-chat" : "w-40"
            }`}
          >
            <ul className="min-h-0 flex-1 space-y-1 overflow-y-auto p-2 text-sm">
              {chatMessages.length === 0 && (
                <li className="text-ink-muted">No messages yet.</li>
              )}
              {chatMessages.map((msg, index) => (
                <li key={index}>
                  <span className="text-ink-muted">
                    {msg.self ? "You" : preferredName(msg.from, members)}:
                  </span>{" "}
                  <span className="text-ink">{msg.text}</span>
                </li>
              ))}
            </ul>
            <form onSubmit={submitChat} className="flex shrink-0 gap-1 border-t p-2">
              <input
                value={chatDraft}
                onChange={(event) => setChatDraft(event.target.value)}
                placeholder="Message everyone"
                className="min-w-0 flex-1 rounded-card border bg-bg px-2 py-1 text-sm text-ink"
              />
              <button
                type="submit"
                disabled={!chatDraft.trim()}
                className="rounded-card border px-2 py-1 text-sm text-brand hover:underline disabled:opacity-50"
              >
                Send
              </button>
            </form>
          </div>
        )}
      </div>
    </section>
  );
}
