import { useState } from "react";
import useMeetingSession from "../hooks/useMeetingSession";
import ControlPanel from "./ControlPanel";
import MeetingGrid from "./MeetingGrid";

/**
 * A joined meeting: video area, overlay controls, and the SDK session
 * behind them, owned by [`useMeetingSession`]. This component only lays out what it reports.
 */
export default function Meeting({ code, token, onLeave }) {
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

  return (
    <div className="relative h-[70vh] min-h-[380px] w-full overflow-hidden rounded-xl border border-slate-800 bg-slate-950">
      <MeetingGrid
        roster={roster}
        focused={focused}
        onFocus={setFocused}
        onBindVideo={bindVideo}
        onMute={(cell) => moderate(cell, "mute")}
        onCameraOff={(cell) => moderate(cell, "camera-off")}
        participantsPosition={settings.participants}
      />

      {status !== "live" && (
        <div className="absolute inset-0 z-20 flex items-center justify-center bg-slate-950/85 p-6">
          <div className="max-w-md text-center">
            <p className="font-mono text-sm text-slate-300">
              {status === "joining" && `joining ${code}...`}
              {status === "ended" && "the meeting ended"}
              {status === "error" && "could not join"}
            </p>
            {error && (
              <p className="mt-3 rounded-lg border border-rose-800 bg-slate-950 p-3 font-mono text-xs break-all text-rose-300">
                {error}
              </p>
            )}
            {status !== "joining" && (
              <button
                type="button"
                onClick={leave}
                className="mt-4 rounded-lg bg-slate-700 px-4 py-1.5 text-sm font-medium text-white hover:bg-slate-600"
              >
                back
              </button>
            )}
          </div>
        </div>
      )}

      {status === "live" && (
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
              className={`absolute top-3 left-1/2 z-10 max-w-[80%] -translate-x-1/2 rounded-lg border bg-slate-950/90 px-3 py-1.5 font-mono text-xs break-all ${
                error
                  ? "border-rose-800 text-rose-300"
                  : "border-slate-700 text-slate-300"
              }`}
            >
              {error || notice}
            </p>
          )}
        </>
      )}

      {/* The SDK plays every remote attendee through this one element. */}
      <audio ref={audioRef} className="hidden" />
    </div>
  );
}
