import ParticipantTile from "./ParticipantTile";

/// While one participant is expanded, the rest sit in a strip: `flow` orders
/// the children, `box` sizes/scrolls the strip, `item` sizes one tile.
const STRIP = {
  top: {
    flow: "flex-col-reverse",
    box: "h-24 flex-row overflow-x-auto sm:h-32",
    item: "h-full aspect-video shrink-0",
  },
  bottom: {
    flow: "flex-col",
    box: "h-24 flex-row overflow-x-auto sm:h-32",
    item: "h-full aspect-video shrink-0",
  },
  left: {
    flow: "flex-row-reverse",
    box: "w-32 flex-col overflow-y-auto sm:w-44",
    item: "w-full aspect-video shrink-0",
  },
  right: {
    flow: "flex-row",
    box: "w-32 flex-col overflow-y-auto sm:w-44",
    item: "w-full aspect-video shrink-0",
  },
};

/// The backend appends a per-join suffix to keep ExternalUserIds unique, and
/// Chime appends `#content` to a screen share's; show the email in front.
function cellLabel({ externalUserId, local, content }) {
  const name = shortLabel(externalUserId);
  if (content) return `${name} — screen`;
  return local ? `${name} (you)` : name;
}

function shortLabel(externalUserId) {
  return String(externalUserId ?? "").split("#")[0] || "attendee";
}

/**
 * One area per attendee present, not per video tile. Plain grid, or — while
 * `focused` names one — that attendee expanded with the rest in a strip.
 */
export default function MeetingGrid({
  roster,
  focused,
  onFocus,
  onBindVideo,
  onMute,
  onCameraOff,
  participantsPosition,
}) {
  const cells = roster.map((entry) => ({
    key: entry.attendeeId,
    label: cellLabel(entry),
    local: entry.local,
    content: entry.content,
    tileId: entry.tileId,
    muted: entry.muted,
    active: entry.active,
  }));

  // An expanded participant who left is no longer expanded.
  const expanded = cells.find((cell) => cell.key === focused) ?? null;
  const others = expanded ? cells.filter((cell) => cell !== expanded) : cells;
  const strip = STRIP[participantsPosition];

  function tileFor(cell, isExpanded) {
    return (
      <ParticipantTile
        cell={cell}
        expanded={isExpanded}
        onToggle={() => onFocus(isExpanded ? null : cell.key)}
        onBindVideo={onBindVideo}
        onMute={() => onMute(cell)}
        onCameraOff={() => onCameraOff(cell)}
      />
    );
  }

  if (expanded) {
    return (
      <div className={`flex h-full w-full gap-2 p-2 ${strip.flow}`}>
        <div className="min-h-0 min-w-0 flex-1">{tileFor(expanded, true)}</div>
        {others.length > 0 && (
          <div className={`flex shrink-0 gap-2 ${strip.box}`}>
            {others.map((cell) => (
              <div key={cell.key} className={strip.item}>
                {tileFor(cell, false)}
              </div>
            ))}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="grid h-full w-full auto-rows-fr grid-cols-[repeat(auto-fit,minmax(220px,1fr))] gap-2 overflow-y-auto p-2">
      {cells.map((cell) => (
        <div key={cell.key} className="min-h-0 min-w-0">
          {tileFor(cell, false)}
        </div>
      ))}
    </div>
  );
}
