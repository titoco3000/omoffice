import { useCallback } from "react";
import {
  IconCameraOff,
  IconCollapse,
  IconExpand,
  IconMicOff,
  IconUser,
} from "../ui/icons";

/**
 * One participant's area, present for every attendee whether or not they
 * have video, so toggling a camera never reshuffles the layout.
 */
export default function ParticipantTile({
  cell,
  expanded,
  onToggle,
  onBindVideo,
  onMute,
  onCameraOff,
  compact = false,
}) {
  const { tileId } = cell;
  const hasVideo = tileId !== null;
  const videoRef = useCallback(
    (element) => onBindVideo(tileId, element),
    [onBindVideo, tileId],
  );

  return (
    <div
      className={`group relative h-full w-full overflow-hidden rounded-lg border bg-muted ${
        cell.active ? "border-success ring-1 ring-success/40" : "border-line"
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
        <div className="flex h-full w-full items-center justify-center bg-muted">
          <IconUser className="h-1/3 max-h-20 min-h-6 w-auto text-ink-faint" />
        </div>
      )}

      {/* Name, plus the mute state the SDK reports for that attendee. */}
      <div className="pointer-events-none absolute bottom-0 left-0 flex max-w-full items-center gap-1.5 rounded-tr-lg bg-surface/80 px-2 py-1 backdrop-blur">
        {cell.muted && !cell.content && (
          <IconMicOff className="h-3.5 w-3.5 shrink-0 text-danger" />
        )}
        {!compact && (
          <span className="truncate font-mono text-xs text-ink-muted">
            {cell.label}
          </span>
        )}
      </div>

      <div
        className={`absolute top-1.5 right-1.5 flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100 ${
          compact ? "hidden" : ""
        }`}
      >
        {/* Moderation only ever takes away; giving it back is theirs alone. */}
        {!cell.local && !cell.content && (
          <>
            <TileButton
              icon={<IconMicOff className="h-4 w-4" />}
              label={cell.muted ? "already muted" : "mute for everyone"}
              onClick={onMute}
              disabled={cell.muted}
            />
            <TileButton
              icon={<IconCameraOff className="h-4 w-4" />}
              label={hasVideo ? "turn their camera off" : "camera already off"}
              onClick={onCameraOff}
              disabled={!hasVideo}
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
      className="rounded-md bg-surface/80 p-1.5 text-ink backdrop-blur hover:bg-muted disabled:cursor-not-allowed disabled:text-ink-faint disabled:hover:bg-surface/80"
    >
      {icon}
    </button>
  );
}
