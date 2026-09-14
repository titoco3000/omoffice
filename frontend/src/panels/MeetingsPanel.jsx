import { useState } from "react";
import { decodeJwt } from "../lib/jwt";
import { preferredName } from "../lib/names";
import * as chime from "../lib/chime";

export default function MeetingsPanel({ token, session, members }) {
  const myId = decodeJwt(token)?.sub;
  const [chatDraft, setChatDraft] = useState("");
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
    create,
    join,
    leave,
    toggleCamera,
    toggleMic,
    toggleScreenShare,
  } = session;

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

  return (
    <section className="rounded-card border bg-surface p-5">
      {/* Chime's audio output -- never rendered as a control, just a sink. */}
      <audio ref={audioRef} autoPlay hidden />

      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-ink">Meetings</h2>
        <button
          onClick={() => create()}
          disabled={!!myMeetingId}
          className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
        >
          New meeting
        </button>
      </div>

      {myMeetingId && (
        <div className="mt-3 rounded-card border bg-bg p-3">
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

          {/* No history behind this -- see `useMeetingSession`'s
              `chatMessages`: nothing is stored server-side, only what this
              tab has sent or received since joining. */}
          <div className="mt-3 rounded-card border bg-surface p-2">
            <ul className="max-h-40 space-y-1 overflow-y-auto text-sm">
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
            <form onSubmit={submitChat} className="mt-2 flex gap-2">
              <input
                value={chatDraft}
                onChange={(event) => setChatDraft(event.target.value)}
                placeholder="Message everyone in this meeting"
                className="flex-1 rounded-card border bg-bg px-2 py-1 text-sm text-ink"
              />
              <button
                type="submit"
                disabled={!chatDraft.trim()}
                className="rounded-card border px-3 py-1 text-sm text-brand hover:underline disabled:opacity-50"
              >
                Send
              </button>
            </form>
          </div>
        </div>
      )}

      {meetings.length === 0 && (
        <p className="mt-3 text-sm text-ink-muted">No meetings right now.</p>
      )}

      <ul className="mt-3 space-y-1.5 text-sm">
        {meetings.map((meeting) => {
          const mine = meeting.id === myMeetingId;
          return (
            <li
              key={meeting.id}
              className="flex items-center justify-between rounded-card border px-3 py-2"
            >
              <span className="text-ink">
                {mine && (
                  // Debug aid: whether this tab actually has a live Chime
                  // session for its own meeting, or is just sitting in the
                  // app-level "waiting" state with no audio connected.
                  <span
                    className={`mr-1.5 inline-block ${roomStatus === "connected" ? "text-success" : "text-ink-muted/40"}`}
                    title={
                      roomStatus === "connected"
                        ? "connected to chime"
                        : "not connected to chime"
                    }
                  >
                    ●
                  </span>
                )}
                {meeting.created_by === myId ? "Your meeting" : "Meeting"}{" "}
                <span className="text-ink-muted">{meeting.id.slice(0, 8)}</span>
                {meeting.attendees?.length > 0 && (
                  <span className="ml-2 text-xs text-ink-muted">
                    {meeting.attendees.length} in it
                  </span>
                )}
                {mine && (
                  <span className="ml-2 text-xs text-ink-muted">
                    {roomStatus === "connected"
                      ? "-- connected"
                      : "-- waiting for others"}
                  </span>
                )}
              </span>
              {mine ? (
                <button onClick={leave} className="text-danger hover:underline">
                  Leave
                </button>
              ) : (
                <button
                  onClick={() => join(meeting.id)}
                  disabled={!!myMeetingId}
                  className="text-brand hover:underline disabled:opacity-50"
                >
                  Join
                </button>
              )}
            </li>
          );
        })}
      </ul>

      {error && <p className="mt-3 text-sm text-danger">{error}</p>}
    </section>
  );
}
