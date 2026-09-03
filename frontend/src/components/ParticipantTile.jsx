import { useCallback } from "react";
import {
  IconCameraOff,
  IconCollapse,
  IconExpand,
  IconMicOff,
  IconUser,
} from "./icons";

/**
 * One participant's area.
 *
 * The area exists for every attendee in the roster, camera or not: a tile that
 * only appeared once video arrived would reshuffle the whole layout every time
 * somebody toggled their camera. `cell.tileId` is null until the SDK reports a
 * video tile for that attendee, and until then the placeholder holds the space.
 *
 * `onBindVideo` goes back to `Meeting`, which owns the SDK binding; this
 * component only reports the element it mounted for the tile.
 */
export default function ParticipantTile({
  cell,
  expanded,
  onToggle,
  onBindVideo,
}) {
  const { tileId } = cell;
  const hasVideo = tileId !== null;
  const videoRef = useCallback(
    (element) => onBindVideo(tileId, element),
    [onBindVideo, tileId],
  );

  return (
    <div
      className={`group relative h-full w-full overflow-hidden rounded-lg border bg-slate-900 ${
        cell.active
          ? "border-emerald-500/70 ring-1 ring-emerald-500/40"
          : "border-slate-800"
      }`}
    >
      {hasVideo ? (
        <video
          ref={videoRef}
          autoPlay
          playsInline
          muted={cell.local}
          className={`h-full w-full bg-black ${
            cell.content ? "object-contain" : "object-cover"
          }`}
        />
      ) : (
        <div className="flex h-full w-full items-center justify-center bg-slate-950">
          <IconUser className="h-1/3 max-h-20 min-h-8 w-auto text-slate-700" />
        </div>
      )}

      {/* Name, plus the mute state the SDK reports for that attendee. */}
      <div className="pointer-events-none absolute bottom-0 left-0 flex max-w-full items-center gap-1.5 rounded-tr-lg bg-slate-950/70 px-2 py-1">
        {cell.muted && !cell.content && (
          <IconMicOff className="h-3.5 w-3.5 shrink-0 text-rose-400" />
        )}
        <span className="truncate font-mono text-xs text-slate-300">
          {cell.label}
        </span>
      </div>

      <div className="absolute top-1.5 right-1.5 flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
        {/* Moderation over another attendee needs a backend signalling path;
            the controls are here so the layout is final, and do nothing yet. */}
        {!cell.local && !cell.content && (
          <>
            <TileButton
              icon={<IconMicOff className="h-4 w-4" />}
              label="mute participant (not wired up yet)"
              disabled
            />
            <TileButton
              icon={<IconCameraOff className="h-4 w-4" />}
              label="stop participant camera (not wired up yet)"
              disabled
            />
          </>
        )}
        <TileButton
          icon={
            expanded ? (
              <IconCollapse className="h-4 w-4" />
            ) : (
              <IconExpand className="h-4 w-4" />
            )
          }
          label={expanded ? "back to grid" : "expand"}
          onClick={onToggle}
        />
      </div>
    </div>
  );
}

function TileButton({ icon, label, onClick, disabled = false }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={label}
      aria-label={label}
      className="rounded-md bg-slate-950/70 p-1.5 text-slate-200 hover:bg-slate-800 disabled:cursor-not-allowed disabled:text-slate-500 disabled:hover:bg-slate-950/70"
    >
      {icon}
    </button>
  );
}
