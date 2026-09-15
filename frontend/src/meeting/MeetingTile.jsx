//! One participant's video tile -- their camera feed if bound, else an
//! initial placeholder. Hovering reveals mute/camera-off buttons; the
//! border turns `speaking` while their mic is producing sound.

import { FiMicOff, FiVideoOff } from "react-icons/fi";

export default function MeetingTile({
  name,
  videoRef,
  hasVideo,
  isSelf,
  speaking,
  wide,
  onMute,
  onCameraOff,
}) {
  return (
    <div
      className={`group relative aspect-video shrink-0 overflow-hidden rounded-card border-2 bg-muted ${
        speaking ? "border-speaking" : "border-transparent"
      } ${wide ? "w-full sm:w-96" : "w-40 sm:w-48"}`}
    >
      {/* The self tile's `<video>` stays mounted regardless of `hasVideo` --
          `useMeetingSession` binds `selfVideoRef` to it once, when the
          meeting starts, not on every camera toggle; unmounting it here
          whenever the camera is off would drop that binding for good, and
          the camera would never come back on. */}
      {isSelf ? (
        <video
          ref={videoRef}
          autoPlay
          muted
          playsInline
          className="size-full -scale-x-100 object-cover"
        />
      ) : hasVideo ? (
        <video
          ref={videoRef}
          autoPlay
          playsInline
          className="size-full object-cover"
        />
      ) : (
        <div className="flex size-full items-center justify-center">
          <span className="flex size-12 items-center justify-center rounded-full bg-brand text-lg font-semibold text-on-brand">
            {name.charAt(0).toUpperCase()}
          </span>
        </div>
      )}

      <span className="absolute bottom-1 left-1.5 rounded bg-black/50 px-1.5 py-0.5 text-xs text-white">
        {name}
      </span>

      {(onMute || onCameraOff) && (
        <div className="absolute inset-0 flex items-center justify-center gap-2 bg-black/40 opacity-0 transition-opacity group-hover:opacity-100">
          {onMute && (
            <button
              onClick={onMute}
              title="Mute"
              className="flex items-center gap-1 rounded-card bg-surface/90 px-2 py-1 text-xs text-ink hover:bg-surface"
            >
              <FiMicOff /> Mute
            </button>
          )}
          {onCameraOff && (
            <button
              onClick={onCameraOff}
              title="Camera off"
              className="flex items-center gap-1 rounded-card bg-surface/90 px-2 py-1 text-xs text-ink hover:bg-surface"
            >
              <FiVideoOff /> Camera off
            </button>
          )}
        </div>
      )}
    </div>
  );
}
