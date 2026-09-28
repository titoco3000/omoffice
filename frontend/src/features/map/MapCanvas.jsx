//! The office map itself: an `OFFICE_SIZE` x `OFFICE_SIZE` grid drawn on a
//! `<canvas>`, one avatar per online org member on its cell. An avatar is a
//! character sprite from `user-sprite.png` (3x3 frames of `SPRITE_SRC_PX`:
//! rows face down/left/up, columns are idle/walk1/walk2; facing right is
//! the left row mirrored) with the member's user icon -- their profile
//! picture, or a first-letter placeholder -- pinned to its top-right. While
//! an avatar's drawn position is moving it faces the dominant axis of that
//! motion and cycles idle -> walk1 -> idle -> walk2, advanced by distance
//! walked rather than time so the legs keep pace with `USER_SPEED`; once it
//! stops it drops back to idle, still facing its last direction. Movement comes
//! from `useMapSession` (arrow keys / click, cooldown-gated); this
//! component's own job is turning `positions` (logical, one integer cell
//! per user) into something that reads as continuous motion -- each
//! avatar's drawn position eases toward its logical cell at `USER_SPEED`
//! cells/sec, along the grid (see `easeAlongGrid`), redrawn every
//! animation frame, rather than jumping the
//! instant a `MAP_MOVE_CHANNEL` push (or this tab's own optimistic move)
//! lands. The exception is someone being *placed* rather than moving --
//! everyone on the initial load, and anyone who comes online later (see
//! `useMapSession`'s `placements`): they're drawn straight on their cell.
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
import spriteUrl from "./user-sprite.png";
import mapUrl from "./map.png";
import grassUrl from "./tiles/grass.png";
import sandUrl from "./tiles/sand.png";
import waterUrl from "./tiles/water.png";
import { ZOOM_STEP, clampCellPx, easeCellPx, useMapZoom } from "./useMapZoom";

// One pixel of `map.png` per cell -- its color picks the terrain drawn
// under that cell. Anything not explicitly grass/sand (including black)
// is water, so an unassigned color reads as water rather than crashing.
const TILE_GRASS = "grass";
const TILE_SAND = "sand";
const TILE_WATER = "water";
const TILE_URLS = {
  [TILE_GRASS]: grassUrl,
  [TILE_SAND]: sandUrl,
  [TILE_WATER]: waterUrl,
};

function tileForColor(r, g, b) {
  if (r === 0 && g === 255 && b === 0) return TILE_GRASS;
  if (r === 255 && g === 255 && b === 0) return TILE_SAND;
  return TILE_WATER;
}

// Sprites are drawn taller than one cell -- a cell is a finer unit of
// movement/terrain than a person's footprint, so the avatar spans this many
// of them.
const SPRITE_CELLS = 1.5;
// At the default zoom, 1.5x the source, so pixel-exact on 2x displays (3
// device px per sprite px).
const SPRITE_SRC_PX = 32;
// The user icon's diameter, as a fraction of a cell -- but never smaller
// than `MIN_USER_ICON_PX`, so it stays a recognizable dot zoomed all the
// way out.
const USER_ICON_CELLS = 1 / 3;
const MIN_USER_ICON_PX = 6;
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

// Moves `current` up to `step` cells toward `target` one axis at a time,
// never diagonally -- a sprite walking diagonally doesn't read as walking.
// Horizontal first, so a click's jump of several cells (applied in one
// move, see `useMapSession`) walks the same L-shaped path on every tab. The exception is an axis already
// mid-step (off its grid line): that one is finished first, rather than
// sliding sideways between two rows.
function easeAlongGrid(current, target, step) {
  const yFirst = Number.isInteger(current.x) && !Number.isInteger(current.y);
  const axes = yFirst ? ["y", "x"] : ["x", "y"];
  const next = { ...current };
  let left = step;
  for (const axis of axes) {
    const distance = Math.abs(target[axis] - next[axis]);
    if (distance <= left) {
      // Assigned, not added, so arriving lands exactly on the grid line.
      next[axis] = target[axis];
      left -= distance;
    } else {
      next[axis] += Math.sign(target[axis] - next[axis]) * left;
      left = 0;
    }
  }
  return next;
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

export default function MapCanvas({ token, orgId, members, myId }) {
  const canvasRef = useRef(null);
  const imagesRef = useRef(new Map()); // url -> HTMLImageElement
  const tileMapRef = useRef(null); // {width, height, tiles: Uint8Array-like of TILE_*} once map.png is decoded
  const renderedRef = useRef({}); // user_id -> {x, y} (eased, float)
  const renderedOffsetRef = useRef({}); // user_id -> {dx, dy} (eased, float)
  const seenPlacementsRef = useRef({}); // user_id -> last placement drawn
  const walkRef = useRef({}); // user_id -> {facing, distance, movedAt}
  // What the last frame was drawn with, for mapping a click back to a cell.
  const viewRef = useRef({ cellPx: 0, originPxX: 0, originPxY: 0 });
  const { cellPxRef, zoomBy, atLimit } = useMapZoom(canvasRef);
  const shownCellPxRef = useRef(null); // eased toward `cellPxRef`

  const { positions, moveTo, loaded, placements } = useMapSession(
    token,
    orgId,
    members,
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
  const stateRef = useRef({ positions, members, myId, loaded, placements });
  useEffect(() => {
    stateRef.current = { positions, members, myId, loaded, placements };
  });

  // Decode `map.png` into a per-cell terrain grid once: an offscreen canvas
  // is the only way to read pixel colors back out of an `<img>`. The draw
  // loop below just polls `tileMapRef` each frame rather than waiting on
  // this, so the grid/avatars still render before the map image loads.
  useEffect(() => {
    const img = new Image();
    img.onload = () => {
      const off = document.createElement("canvas");
      off.width = img.width;
      off.height = img.height;
      const octx = off.getContext("2d");
      octx.drawImage(img, 0, 0);
      const { data } = octx.getImageData(0, 0, off.width, off.height);
      const tiles = new Array(off.width * off.height);
      for (let i = 0; i < tiles.length; i++) {
        tiles[i] = tileForColor(data[i * 4], data[i * 4 + 1], data[i * 4 + 2]);
      }
      tileMapRef.current = { width: off.width, height: off.height, tiles };
    };
    img.src = mapUrl;
  }, []);

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
      const { positions, myId, loaded, placements } = stateRef.current;
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
          continue;
        }
        const current = rendered[member.id] ?? target;
        const next = easeAlongGrid(current, target, USER_SPEED * dt);
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

      const tileMap = tileMapRef.current;
      if (tileMap) {
        ctx.imageSmoothingEnabled = false;
        const tileSizePx = Math.ceil(cellPx) + 1; // 1px overlap so eased scroll never shows a seam
        for (
          let row = Math.max(0, firstRow);
          row < Math.min(tileMap.height, lastRow);
          row++
        ) {
          for (
            let col = Math.max(0, firstCol);
            col < Math.min(tileMap.width, lastCol);
            col++
          ) {
            const tile = tileMap.tiles[row * tileMap.width + col];
            const image = getImage(TILE_URLS[tile]);
            if (!image) continue;
            ctx.drawImage(
              image,
              Math.round(originPxX + col * cellPx),
              Math.round(originPxY + row * cellPx),
              tileSizePx,
              tileSizePx,
            );
          }
        }
      }

      ctx.strokeStyle = border;
      ctx.lineWidth = 1;
      // Faint over the tiles -- a full-opacity grid reads as a fence, not a
      // hint of where cells fall.
      ctx.globalAlpha = tileMap ? 0.01 : 1;
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

      const renderedOffsets = renderedOffsetRef.current;
      for (const group of byCell.values()) {
        group.forEach((member, index) => {
          const pos = rendered[member.id];
          const target = stackOffset(index, group.length);
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
          const bottom = cy + cellPx / 2;
          const top = Math.round((bottom - spritePx) * ratio) / ratio;

          const sprite = getImage(spriteUrl);
          if (sprite) {
            const walk = walkRef.current[member.id];
            const facing = walk?.facing ?? "down";
            const column =
              walk && now - walk.movedAt < WALK_GRACE_MS
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
            ctx.drawImage(
              sprite,
              column * SPRITE_SRC_PX,
              FACING_ROW[facing] * SPRITE_SRC_PX,
              SPRITE_SRC_PX,
              SPRITE_SRC_PX,
              0,
              0,
              spritePx,
              spritePx,
            );
            ctx.restore();
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
        });
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

  const handleClick = (event) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const { cellPx, originPxX, originPxY } = viewRef.current;
    if (!cellPx) return;
    const px = event.clientX - rect.left;
    const py = event.clientY - rect.top;
    const x = Math.floor((px - originPxX) / cellPx);
    const y = Math.floor((py - originPxY) / cellPx);
    moveTo(x, y);
  };

  return (
    <div className="relative size-full">
      <canvas
        ref={canvasRef}
        onClick={handleClick}
        tabIndex={0}
        className="block size-full cursor-pointer touch-none outline-none"
      />
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
