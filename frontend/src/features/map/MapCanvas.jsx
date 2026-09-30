//! The office map itself: an `OFFICE_SIZE` x `OFFICE_SIZE` grid drawn on a
//! `<canvas>`, one avatar per online org member on its cell. An avatar is a
//! character sprite from `user-sprite.png` (3x3 frames of `SPRITE_SRC_PX`:
//! rows face down/left/up, columns are idle/walk1/walk2; facing right is
//! the left row mirrored) with the member's user icon -- their profile
//! picture, or a first-letter placeholder -- pinned to its top-right. While
//! an avatar's drawn position is moving it faces the dominant axis of that
//! motion and cycles idle -> walk1 -> idle -> walk2, advanced by distance
//! walked rather than time so the legs keep pace with `USER_SPEED`; once it
//! stops it drops back to idle, facing the direction its last move reported
//! (`positions[id].facing` -- which is how turning against a wall, with no
//! motion to show, reaches everyone). Movement comes
//! from `useMapSession` (arrow keys / click, cooldown-gated); this
//! component's own job is turning `positions` (logical, one integer cell
//! per user) into something that reads as continuous motion -- each
//! avatar's drawn position eases toward its logical cell at `USER_SPEED`
//! cells/sec, along the shortest route around the walls (`findRoute`, so
//! every tab draws the same walk), redrawn every animation frame, rather
//! than jumping the
//! instant a `MAP_MOVE_CHANNEL` push (or this tab's own optimistic move)
//! lands. The exceptions are someone being *placed* rather than moving --
//! everyone on the initial load, and anyone who comes online later (see
//! `useMapSession`'s `placements`) -- and a move to a cell no route
//! reaches: both are drawn straight on their cell.
//!
//! The camera follows this tab's own (eased) position, centered, so the
//! grid scrolls under the avatar rather than the avatar walking off a
//! fixed viewport -- `OFFICE_SIZE` cells is far more than any one screen
//! fits at a legible size. Near an edge it stops following rather than
//! scroll past the map, and once zoomed out far enough for the map to fit
//! (along either axis) it just centers the map there -- see `viewOrigin`.
//! Zoom (`useMapZoom`) sets the cell size; everything drawn scales with it.

import { useEffect, useRef } from "react";
import { OFFICE_SIZE } from "./api";
import { useMapSession, USER_SPEED } from "./useMapSession";
import { avatarUrl } from "../avatars";
import { FiMinus, FiPlus } from "react-icons/fi";
import spriteUrl from "./assets/user-sprite.png";
import waterEffectUrl from "./assets/effects/water.png";
import { TILE_PX, createTileRenderer, tilePreview } from "./tiles";
import { findRoute } from "./navigation";
import { drawMeetingZones } from "./meetingZones";
import { isWater } from "./tileMap";
import { useMapEditor } from "./MapEditorContext";
import MapEditorPanel from "./MapEditorPanel";
import { ZOOM_STEP, clampCellPx, easeCellPx, useMapZoom } from "./useMapZoom";

// Sprites are drawn taller than one cell -- a cell is a finer unit of
// movement/terrain than a person's footprint, so the avatar spans this many
// of them.
const SPRITE_CELLS = 2;
// At the default zoom, 1.5x the source, so pixel-exact on 2x displays (3
// device px per sprite px).
const SPRITE_SRC_PX = 32;
// The user icon's diameter, as a fraction of a cell -- but never smaller
// than `MIN_USER_ICON_PX`, so it stays a recognizable dot zoomed all the
// way out.
const USER_ICON_CELLS = 1 / 2;
const MIN_USER_ICON_PX = 6;
// Source px cut off the bottom of an avatar standing in water, as if it
// were wading through shallows.
const WATER_CROP_PX = 4;
// The water effect sheet is 16x16: its top and bottom halves are the two
// animation frames, overlaid on the avatar's lower body while in water and
// drawn centered on the sprite at half its width, at the height it had
// when full width.
const WATER_EFFECT_SRC_W = 16;
const WATER_EFFECT_SRC_H = 8;
const WATER_EFFECT_FRAME_MS = 400;
// Source px an avatar on dry land is drawn higher than the tile's bottom
// edge (in water it sits at it, cropped instead).
const LAND_LIFT_PX = 3;
// Spritesheet row per facing; "right" reuses "left", drawn mirrored.
const FACING_ROW = { down: 0, left: 1, up: 2, right: 1 };
// Spritesheet columns, in walk order: idle -> walk1 -> idle -> walk2.
const WALK_CYCLE = [0, 1, 0, 2];
// Cells walked per walk-cycle frame: at `USER_SPEED` 4, eight frames/sec,
// one full cycle every two cells.
const CELLS_PER_WALK_FRAME = 0.5;
// How long an avatar keeps walking after its drawn position last moved --
// a held key's steps can leave a frame or two of standing still between
// one cell's easing finishing and the next move landing, which would
// otherwise flicker to idle mid-walk.
const WALK_GRACE_MS = 120;
// How far apart same-cell avatars fan out from the cell's center, in cells
// -- see the module doc on offsetting stacked members.
const STACK_OFFSET = 0.2;
// How fast a stacked avatar eases to its fan-out spot (cells/sec) when the
// group sharing its cell changes -- someone else arriving or leaving
// reshuffles everyone's slot in `stackOffset`, and snapping straight to the
// new one reads as a jump; easing there instead reads as a small step aside.
const STACK_EASE_PER_SEC = 1;

function cssVar(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name);
}

// Advances `plan` (`{x, y, points, next}`: the route to the cell (x, y),
// and which of its points is being walked to) by up to `step` cells from
// `current`, one axis at a time -- a sprite walking diagonally doesn't read
// as walking. Returns the new position.
function walkPlan(plan, current, step) {
  const pos = { ...current };
  let left = step;
  while (left > 0 && plan.next < plan.points.length) {
    const goal = plan.points[plan.next];
    const dx = goal.x - pos.x;
    const dy = goal.y - pos.y;
    const distance = Math.abs(dx) + Math.abs(dy);
    if (distance <= left) {
      // Assigned, not added, so arriving lands exactly on the grid line.
      pos.x = goal.x;
      pos.y = goal.y;
      left -= distance;
      plan.next++;
    } else {
      pos.x += Math.sign(dx) * left;
      pos.y += Math.sign(dy) * left;
      left = 0;
    }
  }
  return pos;
}

// Canvas px of the map's top-left corner along one axis (`size` px of
// canvas, the camera on cell `camera`): the camera's cell centered, but
// clamped so the map still covers the canvas edge to edge -- or, if the
// whole map fits along this axis, the map centered instead.
function viewOrigin(size, camera, cellPx) {
  const mapPx = OFFICE_SIZE * cellPx;
  if (mapPx <= size) return (size - mapPx) / 2;
  const centered = size / 2 - (camera + 0.5) * cellPx;
  return Math.max(size - mapPx, Math.min(0, centered));
}

// Small deterministic fan-out for members sharing a cell -- same member
// list always produces the same arrangement, rather than reshuffling every
// render.
function stackOffset(index, total) {
  if (total <= 1) return { dx: 0, dy: 0 };
  const angle = (index / total) * Math.PI * 2;
  return {
    dx: Math.cos(angle) * STACK_OFFSET,
    dy: Math.sin(angle) * STACK_OFFSET,
  };
}

export default function MapCanvas({
  token,
  orgId,
  members,
  myId,
  meetings = [],
}) {
  const canvasRef = useRef(null);
  const imagesRef = useRef(new Map()); // url -> HTMLImageElement
  const plansRef = useRef({}); // user_id -> the route being walked, see `walkPlan`
  const renderedRef = useRef({}); // user_id -> {x, y} (eased, float)
  const renderedOffsetRef = useRef({}); // user_id -> {dx, dy} (eased, float)
  const seenPlacementsRef = useRef({}); // user_id -> last placement drawn
  const walkRef = useRef({}); // user_id -> {facing, distance, movedAt}
  // What the last frame was drawn with, for mapping a click back to a cell.
  const viewRef = useRef({ cellPx: 0, originPxX: 0, originPxY: 0 });
  const { cellPxRef, zoomBy, atLimit } = useMapZoom(canvasRef);
  const shownCellPxRef = useRef(null); // eased toward `cellPxRef`
  const hoverRef = useRef(null); // cell under the pointer, for the edit ghost
  const paintingRef = useRef(null); // last cell painted while a drag is on

  const editor = useMapEditor();
  const { tileMap } = editor; // null until the map is loaded and decoded
  const { positions, moveTo, loaded, placements } = useMapSession(
    token,
    orgId,
    members,
    tileMap,
  );

  // The draw loop below reads this every frame instead of closing over
  // `positions`/`members`/`myId` directly -- both `positions` (a fresh
  // object from `useMapSession`'s `useMemo`) and `members` (a fresh array
  // from whatever `.filter()` built it upstream) change reference on
  // essentially every render, including ones with no actual roster change.
  // Keying the canvas-setup effect below on them would restart it that
  // often, and restarting calls `resize()`, which -- per spec -- clears the
  // canvas just from setting `canvas.width`/`height`, even to an unchanged
  // value. With two or more users the extra websocket traffic (both tabs'
  // online/roster pushes) makes that frequent enough that the canvas can go
  // a long stretch, or indefinitely, without a single stable frame to draw.
  const stateRef = useRef({
    positions,
    members,
    myId,
    meetings,
    loaded,
    placements,
    tileMap,
    editing: editor.editing,
    tool: editor.tool,
  });
  useEffect(() => {
    stateRef.current = {
      positions,
      members,
      myId,
      meetings,
      loaded,
      placements,
      tileMap,
      editing: editor.editing,
      tool: editor.tool,
    };
  });

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const getImage = (url) => {
      if (!url) return null;
      let img = imagesRef.current.get(url);
      if (!img) {
        img = new Image();
        img.src = url;
        imagesRef.current.set(url, img);
      }
      return img.complete && img.naturalWidth > 0 ? img : null;
    };

    const tiles = createTileRenderer(getImage);

    let raf;
    let last = performance.now();

    const resize = () => {
      const ratio = window.devicePixelRatio || 1;
      canvas.width = canvas.clientWidth * ratio;
      canvas.height = canvas.clientHeight * ratio;
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);

    const draw = (now) => {
      const { positions, myId, loaded, placements, tileMap } = stateRef.current;
      // Nobody is drawn until the initial positions land -- until then
      // everyone would sit at the (0, 0) default -- nor anyone whose
      // placement is still being fetched.
      const members = loaded
        ? stateRef.current.members.filter(
            (member) => placements[member.id] !== null,
          )
        : [];
      const dt = Math.max(0, Math.min((now - last) / 1000, 0.1));
      last = now;
      const rendered = renderedRef.current;
      const seenPlacements = seenPlacementsRef.current;

      for (const member of members) {
        const target = positions[member.id];
        if (!target) continue;
        const placement = placements[member.id];
        if (placement !== seenPlacements[member.id]) {
          seenPlacements[member.id] = placement;
          rendered[member.id] = { x: target.x, y: target.y };
          delete renderedOffsetRef.current[member.id];
          delete plansRef.current[member.id];
          continue;
        }
        let current = rendered[member.id] ?? target;

        // A new destination (or the map finishing loading) is planned from
        // wherever the avatar has got to -- the same route on every tab.
        let plan = plansRef.current[member.id];
        if (
          !plan ||
          plan.x !== target.x ||
          plan.y !== target.y ||
          plan.tileMap !== tileMap
        ) {
          const route =
            current.x === target.x && current.y === target.y
              ? [current]
              : findRoute(tileMap, current, target);
          // No route: teleport.
          plan = plansRef.current[member.id] = {
            x: target.x,
            y: target.y,
            tileMap,
            points: route ?? [{ x: target.x, y: target.y }],
            next: 1,
          };
          if (!route) current = { x: target.x, y: target.y };
        }
        const next = walkPlan(plan, current, USER_SPEED * dt);
        rendered[member.id] = next;

        const mx = next.x - current.x;
        const my = next.y - current.y;
        if (mx || my) {
          const walk = (walkRef.current[member.id] ??= {
            facing: "down",
            distance: 0,
            movedAt: 0,
          });
          walk.facing =
            Math.abs(mx) > Math.abs(my)
              ? mx > 0
                ? "right"
                : "left"
              : my > 0
                ? "down"
                : "up";
          walk.distance += Math.hypot(mx, my);
          walk.movedAt = now;
        }
      }

      const ctx = canvas.getContext("2d");
      const ratio = window.devicePixelRatio || 1;
      const w = canvas.width / ratio;
      const h = canvas.height / ratio;
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);

      const bg = cssVar("--color-bg") || "#0b0f19";
      const border = cssVar("--color-line") || "#26314a";
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, w, h);

      // Re-clamped every frame, not just when zooming: the limits depend
      // on the canvas size, which a resize changes under it.
      const targetCellPx = clampCellPx(cellPxRef.current, w, h);
      const cellPx =
        shownCellPxRef.current === null
          ? targetCellPx
          : easeCellPx(shownCellPxRef.current, targetCellPx, dt);
      shownCellPxRef.current = cellPx;
      const camera = rendered[myId] ?? { x: 0, y: 0 };
      const originPxX = viewOrigin(w, camera.x, cellPx);
      const originPxY = viewOrigin(h, camera.y, cellPx);
      viewRef.current = { cellPx, originPxX, originPxY };

      // Grid lines, only across whatever range is actually on screen.
      const firstCol = Math.floor(-originPxX / cellPx);
      const lastCol = Math.ceil((w - originPxX) / cellPx);
      const firstRow = Math.floor(-originPxY / cellPx);
      const lastRow = Math.ceil((h - originPxY) / cellPx);

      // Runs each pass over every visible cell in rows `fromRow` up to (not
      // including) `toRow`, in order.
      const paintCells = (passes, fromRow = 0, toRow = tileMap.height) => {
        ctx.imageSmoothingEnabled = false;
        const rows = [
          Math.max(0, firstRow, fromRow),
          Math.min(tileMap.height, lastRow, toRow),
        ];
        const cols = [Math.max(0, firstCol), Math.min(tileMap.width, lastCol)];
        // Cell edges are rounded from the exact position, to whole *device*
        // pixels, so neighbors share one boundary and eased scroll never
        // shows a seam -- at a fractional pixel ratio, rounding in CSS px
        // leaves edges mid-device-pixel, antialiased against the background.
        const edgeX = (col) =>
          Math.round((originPxX + col * cellPx) * ratio) / ratio;
        const edgeY = (row) =>
          Math.round((originPxY + row * cellPx) * ratio) / ratio;
        for (const pass of passes) {
          for (let row = rows[0]; row < rows[1]; row++) {
            for (let col = cols[0]; col < cols[1]; col++) {
              pass(
                ctx,
                tileMap,
                col,
                row,
                edgeX(col),
                edgeY(row),
                edgeX(col + 1),
                edgeY(row + 1),
              );
            }
          }
        }
      };

      if (tileMap) {
        paintCells([tiles.drawBase, tiles.drawTransitions]);
      }

      // Just above the ground, under decorations and avatars.
      drawMeetingZones(
        ctx,
        stateRef.current.meetings,
        rendered,
        new Set(members.map((member) => member.id)),
        { originPxX, originPxY, cellPx },
        {
          accent: cssVar("--color-brand") || "#5b8def",
          surface: cssVar("--color-surface") || "#111827",
        },
      );

      if (tileMap) {
        paintCells([tiles.drawMid]);
      }

      ctx.strokeStyle = border;
      ctx.lineWidth = 1;
      // Faint over the tiles -- a full-opacity grid reads as a fence, not a
      // hint of where cells fall.
      ctx.globalAlpha = tileMap ? 0.015 : 1;
      ctx.beginPath();
      for (
        let col = Math.max(0, firstCol);
        col <= Math.min(OFFICE_SIZE, lastCol);
        col++
      ) {
        const x = Math.round(originPxX + col * cellPx) + 0.5;
        ctx.moveTo(x, Math.max(0, originPxY));
        ctx.lineTo(x, Math.min(h, originPxY + OFFICE_SIZE * cellPx));
      }
      for (
        let row = Math.max(0, firstRow);
        row <= Math.min(OFFICE_SIZE, lastRow);
        row++
      ) {
        const y = Math.round(originPxY + row * cellPx) + 0.5;
        ctx.moveTo(Math.max(0, originPxX), y);
        ctx.lineTo(Math.min(w, originPxX + OFFICE_SIZE * cellPx), y);
      }
      ctx.stroke();
      ctx.globalAlpha = 1;

      // Group members by cell for the stacking offset.
      const byCell = new Map();
      for (const member of members) {
        const pos = rendered[member.id];
        if (!pos) continue;
        const key = `${Math.round(positions[member.id]?.x ?? pos.x)},${Math.round(
          positions[member.id]?.y ?? pos.y,
        )}`;
        if (!byCell.has(key)) byCell.set(key, []);
        byCell.get(key).push(member);
      }

      // Avatars are two cells tall, so they draw top to bottom, with the
      // top level's rows woven in: an avatar standing on a row above a top
      // decoration's is covered by it, one on or below that row draws over it.
      const drawOrder = [...byCell.values()]
        .flatMap((group) =>
          group.map((member, index) => ({
            member,
            index,
            count: group.length,
          })),
        )
        .sort((a, b) => rendered[a.member.id].y - rendered[b.member.id].y);
      let topRow = 0; // top level rows below this are already painted

      const renderedOffsets = renderedOffsetRef.current;
      for (const { member, index, count } of drawOrder) {
        if (tileMap) {
          const row = Math.round(rendered[member.id].y);
          paintCells([tiles.drawTop], topRow, row);
          topRow = Math.max(topRow, row);
        }
        const pos = rendered[member.id];
        const target = stackOffset(index, count);
        const current = renderedOffsets[member.id] ?? target;
        const odx = target.dx - current.dx;
        const ody = target.dy - current.dy;
        const odist = Math.hypot(odx, ody);
        const maxOffsetStep = STACK_EASE_PER_SEC * dt;
        const { dx, dy } =
          odist <= maxOffsetStep
            ? target
            : {
                dx: current.dx + (odx / odist) * maxOffsetStep,
                dy: current.dy + (ody / odist) * maxOffsetStep,
              };
        renderedOffsets[member.id] = { dx, dy };
        const cx = originPxX + (pos.x + 0.5 + dx) * cellPx;
        const cy = originPxY + (pos.y + 0.5 + dy) * cellPx;
        const spritePx = cellPx * SPRITE_CELLS;
        const iconPx = Math.max(MIN_USER_ICON_PX, cellPx * USER_ICON_CELLS);

        // Snapped to whole device pixels so the (unsmoothed) sprite
        // doesn't shimmer as it eases between cells. Anchored to the
        // bottom of the tile (feet on the ground) rather than centered,
        // so a sprite taller than one cell grows upward out of it instead
        // of sinking half into the tile behind.
        const left = Math.round((cx - spritePx / 2) * ratio) / ratio;
        const inWater = isWater(tileMap, Math.round(pos.x), Math.round(pos.y));
        const lift = inWater ? 0 : (spritePx * LAND_LIFT_PX) / SPRITE_SRC_PX;
        const bottom = cy + cellPx / 2 - lift;
        const top = Math.round((bottom - spritePx) * ratio) / ratio;

        const sprite = getImage(spriteUrl);
        if (sprite) {
          const walk = walkRef.current[member.id];
          const moving = walk && now - walk.movedAt < WALK_GRACE_MS;
          const declared = positions[member.id]?.facing;
          // Walking faces where it's going; at rest, whichever way the
          // last move said (which is how turning in place shows up).
          const facing =
            (moving ? walk.facing : declared in FACING_ROW && declared) ||
            walk?.facing ||
            "down";
          const column = moving
            ? WALK_CYCLE[
                Math.floor(walk.distance / CELLS_PER_WALK_FRAME) %
                  WALK_CYCLE.length
              ]
            : 0;
          ctx.save();
          ctx.imageSmoothingEnabled = false;
          if (facing === "right") {
            ctx.translate(left + spritePx, top);
            ctx.scale(-1, 1);
          } else {
            ctx.translate(left, top);
          }
          const cropPx = inWater ? WATER_CROP_PX : 0;
          ctx.drawImage(
            sprite,
            column * SPRITE_SRC_PX,
            FACING_ROW[facing] * SPRITE_SRC_PX,
            SPRITE_SRC_PX,
            SPRITE_SRC_PX - cropPx,
            0,
            0,
            spritePx,
            (spritePx * (SPRITE_SRC_PX - cropPx)) / SPRITE_SRC_PX,
          );
          ctx.restore();

          const effect = inWater && getImage(waterEffectUrl);
          if (effect) {
            const frame = Math.floor(now / WATER_EFFECT_FRAME_MS) % 2;
            const effectH =
              (spritePx * WATER_EFFECT_SRC_H) / WATER_EFFECT_SRC_W / 2;
            ctx.save();
            ctx.imageSmoothingEnabled = false;
            ctx.drawImage(
              effect,
              0,
              frame * WATER_EFFECT_SRC_H,
              WATER_EFFECT_SRC_W,
              WATER_EFFECT_SRC_H,
              left + spritePx / 4,
              top +
                (spritePx * (SPRITE_SRC_PX - cropPx)) / SPRITE_SRC_PX -
                effectH,
              spritePx / 2,
              effectH,
            );
            ctx.restore();
          }
        }

        const ix = left + spritePx - iconPx / 2;
        const iy = top + iconPx / 2;
        const image = getImage(avatarUrl(member.picture));
        ctx.save();
        ctx.beginPath();
        ctx.arc(ix, iy, iconPx / 2, 0, Math.PI * 2);
        ctx.closePath();
        if (image) {
          ctx.clip();
          ctx.drawImage(
            image,
            ix - iconPx / 2,
            iy - iconPx / 2,
            iconPx,
            iconPx,
          );
        } else {
          ctx.fillStyle =
            member.id === myId
              ? cssVar("--color-brand") || "#5b8def"
              : cssVar("--color-muted") || "#3a4763";
          ctx.fill();
          ctx.fillStyle = "#fff";
          ctx.font = `600 ${Math.round(iconPx * 0.56)}px sans-serif`;
          ctx.textAlign = "center";
          ctx.textBaseline = "middle";
          ctx.fillText((member.name || "?").charAt(0).toUpperCase(), ix, iy);
        }
        ctx.restore();
        ctx.lineWidth = member.id === myId ? 2 : 1;
        ctx.strokeStyle =
          member.id === myId ? cssVar("--color-brand") || "#5b8def" : bg;
        ctx.stroke();
      }

      if (tileMap) paintCells([tiles.drawTop], topRow);

      // Edit mode: the selected tile as a ghost under the pointer.
      const { editing, tool } = stateRef.current;
      const hover = hoverRef.current;
      if (editing && hover) {
        const gx = originPxX + hover.x * cellPx;
        const gy = originPxY + hover.y * cellPx;
        ctx.save();
        ctx.imageSmoothingEnabled = false;
        const preview = tool.tile && tilePreview(tool.tile);
        const ghost = preview && getImage(preview.url);
        if (ghost) {
          ctx.globalAlpha = 0.6;
          if (preview.src) {
            ctx.drawImage(ghost, ...preview.src, gx, gy, cellPx, cellPx);
          } else {
            ctx.drawImage(
              ghost,
              gx,
              gy,
              (ghost.naturalWidth * cellPx) / TILE_PX,
              (ghost.naturalHeight * cellPx) / TILE_PX,
            );
          }
          ctx.globalAlpha = 1;
        }
        ctx.lineWidth = 2;
        ctx.strokeStyle = tool.tile
          ? cssVar("--color-brand") || "#5b8def"
          : cssVar("--color-danger") || "#e5484d";
        ctx.strokeRect(gx + 1, gy + 1, cellPx - 2, cellPx - 2);
        ctx.restore();
      }

      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
    };
    // Deliberately mount-only -- see `stateRef` above. (`cellPxRef` is a
    // ref, so listing it never re-runs this.)
  }, [cellPxRef]);

  // The map cell under a pointer event, `null` off the map.
  const cellAt = (event) => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    const { cellPx, originPxX, originPxY } = viewRef.current;
    if (!cellPx) return null;
    const x = Math.floor((event.clientX - rect.left - originPxX) / cellPx);
    const y = Math.floor((event.clientY - rect.top - originPxY) / cellPx);
    if (x < 0 || y < 0 || x >= OFFICE_SIZE || y >= OFFICE_SIZE) return null;
    return { x, y };
  };

  const handleClick = (event) => {
    if (editor.editing) return;
    const cell = cellAt(event);
    if (cell) moveTo(cell.x, cell.y);
  };

  // Edit mode: a press paints its cell, dragging paints every cell along
  // the way (a fast drag skips cells, so the gaps are filled in as a line).
  const handlePointerDown = (event) => {
    if (!editor.editing || !event.isPrimary || event.button !== 0) return;
    const cell = cellAt(event);
    if (!cell) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    paintingRef.current = cell;
    editor.paint(cell, cell);
  };

  const handlePointerMove = (event) => {
    if (!editor.editing) return;
    const cell = cellAt(event);
    hoverRef.current = cell;
    if (cell && paintingRef.current) {
      editor.paint(paintingRef.current, cell);
      paintingRef.current = cell;
    }
  };

  const endPaint = () => {
    paintingRef.current = null;
  };

  return (
    <div className="relative size-full">
      <canvas
        ref={canvasRef}
        onClick={handleClick}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endPaint}
        onPointerCancel={endPaint}
        onPointerLeave={() => {
          hoverRef.current = null;
        }}
        tabIndex={0}
        className={`block size-full touch-none outline-none ${
          editor.editing ? "cursor-crosshair" : "cursor-pointer"
        }`}
      />
      {editor.editing && <MapEditorPanel />}
      <div className="absolute right-4 top-4 flex flex-col overflow-hidden rounded-card border bg-surface shadow-lg">
        <button
          onClick={() => zoomBy(ZOOM_STEP)}
          disabled={atLimit.max}
          title="Zoom in"
          className="flex size-9 items-center justify-center text-ink-muted enabled:hover:text-ink disabled:opacity-40"
        >
          <FiPlus />
        </button>
        <button
          onClick={() => zoomBy(1 / ZOOM_STEP)}
          disabled={atLimit.min}
          title="Zoom out"
          className="flex size-9 items-center justify-center border-t text-ink-muted enabled:hover:text-ink disabled:opacity-40"
        >
          <FiMinus />
        </button>
      </div>
    </div>
  );
}
