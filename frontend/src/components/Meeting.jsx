import { useState } from "react";
import useMeetingSession from "../hooks/useMeetingSession";
import { usePresence } from "../messenger/MessengerContext";
import Button from "../ui/Button";
import { IconExpand, IconLeave } from "../ui/icons";
import ControlPanel from "./ControlPanel";
import MeetingGrid from "./MeetingGrid";

/**
 * A joined meeting: video area, overlay controls, and the SDK session
 * behind them, owned by [`useMeetingSession`]. This component only lays out
 * what it reports.
 *
 * `mini` is the same session drawn small, for when the page under it is not
 * the meeting page — same DOM, same tiles, fewer controls.
 */
export default function Meeting({ code, token, onLeave, mini = false, onExpand }) {
  const [settings, setSettings] = useState({
    panel: "bottom left",
    participants: "right",
  });

  const {
    status,
    error,
    notice,
    roster,
    muted,
    cameraOn,
    sharing,
    focused,
    setFocused,
    audioRef,
    bindVideo,
    moderate,
    leave,
    toggleMute,
    toggleCamera,
    toggleShare,
  } = useMeetingSession({ code, token, onLeave });

  // Being in this meeting is part of what this tab reports about itself,
  // and what `Messenger::send_to_users_on_meeting` addresses.
  usePresence({ meeting: code });

  return (
    <div className="relative h-full w-full overflow-hidden rounded-card border border-line bg-bg shadow-lg">
      <MeetingGrid
        roster={roster}
        focused={mini ? null : focused}
        onFocus={setFocused}
        onBindVideo={bindVideo}
        onMute={(cell) => moderate(cell, "mute")}
        onCameraOff={(cell) => moderate(cell, "camera-off")}
        participantsPosition={settings.participants}
        compact={mini}
      />

      {status !== "live" && (
        <div className="absolute inset-0 z-20 flex items-center justify-center bg-bg/90 p-4">
          <div className="max-w-md text-center">
            <p className="font-mono text-sm text-ink-muted">
              {status === "waiting" && "waiting for someone else to join..."}
              {status === "joining" && `joining ${code}...`}
              {status === "ended" && "the meeting ended"}
              {status === "error" && "could not join"}
            </p>
            {error && !mini && (
              <p className="mt-3 rounded-lg border border-danger/40 bg-danger/10 p-3 font-mono text-xs break-all text-danger">
                {error}
              </p>
            )}
            {status !== "joining" && (
              <Button variant="secondary" size="sm" className="mt-4" onClick={leave}>
                close
              </Button>
            )}
          </div>
        </div>
      )}

      {status === "live" && !mini && (
        <>
          <ControlPanel
            settings={settings}
            onSettings={setSettings}
            muted={muted}
            onToggleMute={toggleMute}
            cameraOn={cameraOn}
            onToggleCamera={toggleCamera}
            sharing={sharing}
            onToggleShare={toggleShare}
            onLeave={leave}
          />
          {(error || notice) && (
            <p
              className={`absolute top-3 left-1/2 z-10 max-w-[80%] -translate-x-1/2 rounded-lg border bg-surface/90 px-3 py-1.5 font-mono text-xs break-all backdrop-blur ${
                error ? "border-danger/40 text-danger" : "border-line text-ink-muted"
              }`}
            >
              {error || notice}
            </p>
          )}
        </>
      )}

      {/* Small: only the two things worth doing from another page. */}
      {mini && (
        <div className="absolute inset-x-1 bottom-1 z-10 flex items-center justify-end gap-1">
          <Button
            variant="secondary"
            size="icon-sm"
            title="back to the meeting"
            aria-label="back to the meeting"
            onClick={onExpand}
          >
            <IconExpand className="h-4 w-4" />
          </Button>
          <Button
            variant="danger"
            size="icon-sm"
            title="leave"
            aria-label="leave"
            onClick={leave}
          >
            <IconLeave className="h-4 w-4" />
          </Button>
        </div>
      )}

      {/* The SDK plays every remote attendee through this one element. */}
      <audio ref={audioRef} className="hidden" />
    </div>
  );
}
