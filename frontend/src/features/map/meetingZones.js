//! The translucent polygon drawn under everyone who shares a meeting, so it
//! reads on the map that they are together -- with a lock badge on its top
//! right corner when the meeting is locked (`invite_only`).
//!
//! The polygon is the convex hull of a padded box around each attendee's
//! drawn (eased) position, so it follows them as they walk. The box reaches
//! a cell above the attendee too, since sprites are taller than a cell.

// Cells of breathing room around each attendee, and how far the box
// reaches up (the sprite's height beyond its own cell).
const PADDING = 0.4;
const SPRITE_REACH = 1;

// Convex hull, Andrew's monotone chain. Returns the points counter-clockwise
// (in y-down screen space, clockwise on screen); fewer than 3 points come
// back unchanged.
function convexHull(points) {
  if (points.length < 3) return points;
  const sorted = [...points].sort((a, b) => a.x - b.x || a.y - b.y);
  const cross = (o, a, b) =>
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const build = (pts) => {
    const hull = [];
    for (const p of pts) {
      while (
        hull.length >= 2 &&
        cross(hull[hull.length - 2], hull[hull.length - 1], p) <= 0
      )
        hull.pop();
      hull.push(p);
    }
    hull.pop();
    return hull;
  };
  return [...build(sorted), ...build(sorted.reverse())];
}

// One meeting's polygon in cell coordinates, from its attendees' positions.
export function zonePolygon(positions) {
  const corners = [];
  for (const { x, y } of positions) {
    for (const cx of [x - PADDING, x + 1 + PADDING])
      for (const cy of [y - SPRITE_REACH - PADDING, y + 1 + PADDING])
        corners.push({ x: cx, y: cy });
  }
  return convexHull(corners);
}

// The hull's top-right vertex -- furthest right and up, so the badge sits
// on the shape even when the hull doesn't reach its bounding box's corner.
function topRight(polygon) {
  return polygon.reduce((best, p) => (p.x - p.y > best.x - best.y ? p : best));
}

function drawLock(ctx, cx, cy, radius, colors) {
  ctx.save();
  ctx.beginPath();
  ctx.arc(cx, cy, radius, 0, Math.PI * 2);
  ctx.fillStyle = colors.surface;
  ctx.fill();
  ctx.lineWidth = Math.max(1, radius * 0.12);
  ctx.strokeStyle = colors.accent;
  ctx.stroke();

  // Body, with the shackle arching over it.
  const bodyW = radius * 0.9;
  const bodyH = radius * 0.65;
  const bodyX = cx - bodyW / 2;
  const bodyY = cy - bodyH / 2 + radius * 0.18;
  const shackleR = bodyW * 0.32;
  ctx.lineWidth = Math.max(1, radius * 0.16);
  ctx.beginPath();
  ctx.arc(cx, bodyY, shackleR, Math.PI, 0);
  ctx.stroke();
  ctx.fillStyle = colors.accent;
  ctx.beginPath();
  ctx.roundRect(bodyX, bodyY, bodyW, bodyH, radius * 0.12);
  ctx.fill();
  ctx.restore();
}

// Draws every meeting's zone. `meetings` is the session's list
// (`{id, attendees, invite_only}`), `rendered` maps user id to drawn cell
// position, `visible` is the set of user ids actually on the map. `view` is
// `{originPxX, originPxY, cellPx}`.
export function drawMeetingZones(
  ctx,
  meetings,
  rendered,
  visible,
  view,
  colors,
) {
  const { originPxX, originPxY, cellPx } = view;
  for (const meeting of meetings) {
    const positions = (meeting.attendees ?? [])
      .filter((id) => visible.has(id) && rendered[id])
      .map((id) => rendered[id]);
    if (positions.length === 0) continue;

    const polygon = zonePolygon(positions).map((p) => ({
      x: originPxX + p.x * cellPx,
      y: originPxY + p.y * cellPx,
    }));

    ctx.save();
    ctx.beginPath();
    polygon.forEach((p, i) =>
      i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y),
    );
    ctx.closePath();
    ctx.lineJoin = "round";
    ctx.globalAlpha = 0.18;
    ctx.fillStyle = colors.accent;
    ctx.fill();
    ctx.globalAlpha = 0.7;
    ctx.lineWidth = 2;
    ctx.setLineDash([cellPx * 0.4, cellPx * 0.25]);
    ctx.strokeStyle = colors.accent;
    ctx.stroke();
    ctx.restore();

    if (meeting.invite_only) {
      const corner = topRight(polygon);
      const radius = Math.max(8, cellPx * 0.45);
      drawLock(ctx, corner.x, corner.y, radius, colors);
    }
  }
}
