//! One participant's video tile -- their camera feed if bound, else their
//! preferred picture (org-scoped, falling back to global -- see
//! `preferredPicture` in `features/org/names.js`), else an initial placeholder.
//! Hovering reveals mute/camera-off/pin buttons; the border turns
//! `speaking` while their mic is producing sound.
//!
//! The self tile's `<video>` stays mounted regardless of `hasVideo` (see
//! below), so its picture/initial placeholder is overlaid on top rather
//! than swapped in for it, unlike every other tile.
//!
//! `fill` sizes the tile to whatever its parent grid cell/flex slot gives
//! it (the gallery grid, and the pinned tile itself) rather than the fixed
//! small size used for the sidebar list of unpinned tiles once something
//! is pinned -- see `MeetingPanel`.

import { useRef } from "react";
import { FiMaximize, FiMicOff, FiMinimize, FiVideoOff } from "react-icons/fi";
import { useIsTouch } from "../../../core/hooks/useViewport";
import BoardCanvas from "./BoardCanvas";

export default function MeetingTile({
  name,
  initial,
  picture,
  videoRef,
  hasVideo,
  isSelf,
  speaking,
  muted,
  pinned,
  fill,
  isContent,
  meetingId,
  ownerId,
  myId,
  onMute,
  onCameraOff,
  onPin,
}) {
  const isTouch = useIsTouch();
  const hasControls = onMute || onCameraOff || onPin;
  // A plain ref to the actual `<video>` element, alongside whatever ref the
  // caller passed in (Chime's own bind callback) -- `BoardCanvas` reads
  // `videoWidth`/`videoHeight` off it directly to work out the shared
  // screen's letterboxed rect. Harmless to keep on every tile; only content
  // tiles actually use it.
  const videoElRef = useRef(null);
  const bindVideoNode = (el) => {
    videoElRef.current = el;
    videoRef?.(el);
  };

  // What shows in place of a camera feed: the user's own preferred picture
  // (org-scoped, already falling back to the global one -- see
  // `core::rbac::picture_key`), circular-cropped same as `Avatar`, else an
  // initial -- `initial` defaults to `name`, but the self tile's `name` is
  // the label "You" rather than an actual name, so it passes its own
  // preferred name in separately just for this.
  const placeholder = (
    <div className="flex size-full items-center justify-center">
      {picture ? (
        <img
          src={picture}
          alt=""
          className="size-12 rounded-full object-cover"
        />
      ) : (
        <span className="flex size-12 items-center justify-center rounded-full bg-brand text-lg font-semibold text-on-brand">
          {(initial ?? name).charAt(0).toUpperCase()}
        </span>
      )}
    </div>
  );
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
        <>
          <video
            ref={videoRef}
            autoPlay
            muted
            playsInline
            className={`size-full -scale-x-100 object-cover ${hasVideo ? "" : "invisible"}`}
          />
          {!hasVideo && <div className="absolute inset-0">{placeholder}</div>}
        </>
      ) : hasVideo ? (
        <>
          <video
            ref={bindVideoNode}
            autoPlay
            playsInline
            className={`size-full ${isContent ? "object-contain" : "object-cover"}`}
          />
          {isContent && (
            <BoardCanvas
              meetingId={meetingId}
              ownerId={ownerId}
              myId={myId}
              videoRef={videoElRef}
            />
          )}
        </>
      ) : (
        placeholder
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
          // `pointer-events-none` on the wrapper -- it's `inset-0` same as
          // `BoardCanvas`'s own overlay, so left unset it would eat every
          // click over the whole tile (blank corners included), not just
          // its buttons, and a content tile's board toolbar/canvas sit
          // underneath it in stacking order. Each button opts back in.
          className={`pointer-events-none absolute inset-0 flex items-center justify-center gap-2 bg-black/40 opacity-0 transition-opacity ${
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
              className="pointer-events-auto flex items-center gap-1 rounded-card bg-surface/90 px-2 py-1 text-xs text-ink hover:bg-surface disabled:opacity-50 disabled:hover:bg-surface/90"
            >
              <FiMicOff />
            </button>
          )}
          {onCameraOff && (
            <button
              onClick={onCameraOff}
              disabled={!hasVideo}
              title="Camera off"
              className="pointer-events-auto flex items-center gap-1 rounded-card bg-surface/90 px-2 py-1 text-xs text-ink hover:bg-surface disabled:opacity-50 disabled:hover:bg-surface/90"
            >
              <FiVideoOff />
            </button>
          )}
          {onPin && (
            <button
              onClick={onPin}
              title={pinned ? "Unpin" : "Pin"}
              className={`pointer-events-auto flex items-center gap-1 rounded-card bg-surface/90 px-2 py-1 text-xs hover:bg-surface ${
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
