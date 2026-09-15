//! One participant's video tile -- their camera feed if bound, else an
//! initial placeholder. Hovering reveals mute/camera-off/pin buttons; the
//! border turns `speaking` while their mic is producing sound.
//!
//! `fill` sizes the tile to whatever its parent grid cell/flex slot gives
//! it (the gallery grid, and the pinned tile itself) rather than the fixed
//! small size used for the sidebar list of unpinned tiles once something
//! is pinned -- see `MeetingPanel`.

import { FiMaximize, FiMicOff, FiMinimize, FiVideoOff } from "react-icons/fi";
import { useIsTouch } from "../layout/useViewport";

export default function MeetingTile({
  name,
  videoRef,
  hasVideo,
  isSelf,
  speaking,
  muted,
  pinned,
  fill,
  onMute,
  onCameraOff,
  onPin,
}) {
  const isTouch = useIsTouch();
  const hasControls = onMute || onCameraOff || onPin;
  return (
    <div
      // `tabIndex` only on touch, where there's no `:hover` -- lets a tap
      // focus the tile and reveal the controls via `focus-within` instead.
      // A mouse/trackpad keeps `:hover` only, so tabbing through with a
      // keyboard doesn't flash the overlay on every tile it passes.
      tabIndex={isTouch && hasControls ? 0 : undefined}
      // `aspect-video` stays on in `fill` mode too -- it's a no-op wherever
      // the parent grid/flex slot actually gives this tile a real height
      // (a definite `height: 100%` wins over `aspect-ratio`), but it's what
      // keeps the tile from collapsing to 0 tall in a spot with no definite
      // height of its own (the floating mini dock, whose card is sized to
      // its content rather than to the viewport) -- a percentage height
      // against an indefinite container resolves to `auto`, so the aspect
      // ratio is what ends up sizing it instead.
      className={`group relative overflow-hidden rounded-card border-2 bg-muted aspect-video ${
        speaking ? "border-speaking" : "border-transparent"
      } ${fill ? "h-full w-full" : "w-40 shrink-0 sm:w-48"}`}
    >
      {/* The self tile's `<video>` stays mounted regardless of `hasVideo` --
          `selfVideoRef` (a callback ref, see `useMeetingSession`) rebinds
          Chime to it on every mount, so unmounting it here whenever the
          camera is off would just have it rebind to nothing, and the
          camera would never come back on. */}
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
      {muted && (
        <span className="absolute bottom-1 right-1 flex size-5 items-center justify-center rounded-full bg-black/50 text-white">
          <FiMicOff size={12} />
        </span>
      )}

      {hasControls && (
        <div
          className={`absolute inset-0 flex items-center justify-center gap-2 bg-black/40 opacity-0 transition-opacity ${
            isTouch
              ? "group-focus-within:opacity-100"
              : "group-hover:opacity-100"
          }`}
        >
          {onMute && (
            <button
              onClick={onMute}
              disabled={muted}
              title="Mute"
              className="flex items-center gap-1 rounded-card bg-surface/90 px-2 py-1 text-xs text-ink hover:bg-surface disabled:opacity-50 disabled:hover:bg-surface/90"
            >
              <FiMicOff />
            </button>
          )}
          {onCameraOff && (
            <button
              onClick={onCameraOff}
              disabled={!hasVideo}
              title="Camera off"
              className="flex items-center gap-1 rounded-card bg-surface/90 px-2 py-1 text-xs text-ink hover:bg-surface disabled:opacity-50 disabled:hover:bg-surface/90"
            >
              <FiVideoOff />
            </button>
          )}
          {onPin && (
            <button
              onClick={onPin}
              title={pinned ? "Unpin" : "Pin"}
              className={`flex items-center gap-1 rounded-card bg-surface/90 px-2 py-1 text-xs hover:bg-surface ${
                pinned ? "text-brand" : "text-ink"
              }`}
            >
              {pinned ? <FiMinimize /> : <FiMaximize />}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
