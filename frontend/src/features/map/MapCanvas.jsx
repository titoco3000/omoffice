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
//! cells/sec, redrawn every animation frame, rather than jumping the
//! instant a `MAP_MOVE_CHANNEL` push (or this tab's own optimistic move)
//! lands. The exception is someone being *placed* rather than moving --
//! everyone on the initial load, and anyone who comes online later (see
//! `useMapSession`'s `placements`): they're drawn straight on their cell.
//!
//! The camera follows this tab's own (eased) position, centered, so the
//! grid scrolls under the avatar rather than the avatar walking off a
//! fixed viewport -- `OFFICE_SIZE` cells is far more than any one screen
//! fits at a legible size.

import { useEffect, useRef } from "react";
import { OFFICE_SIZE } from "./api";
import { useMapSession, USER_SPEED } from "./useMapSession";
import { avatarUrl } from "../avatars";
import spriteUrl from "./user-sprite.png";

const CELL_PX = 48;
const SPRITE_SRC_PX = 32;
// Drawn a whole cell tall -- 1.5x the source, so pixel-exact on 2x
// displays (3 device px per sprite px).
const SPRITE_PX = CELL_PX;
const USER_ICON_PX = 16;
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
// How far apart same-cell avatars fan out from the cell's center -- see the
// module doc on offsetting stacked members.
const STACK_OFFSET_PX = 10;
// How fast a stacked avatar eases to its fan-out spot (px/sec) when the
// group sharing its cell changes -- someone else arriving or leaving
// reshuffles everyone's slot in `stackOffset`, and snapping straight to the
// new one reads as a jump; easing there instead reads as a small step aside.
const STACK_EASE_PX_PER_SEC = 50;

function cssVar(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name);
}

// Small deterministic fan-out for members sharing a cell -- same member
// list always produces the same arrangement, rather than reshuffling every
// render.
function stackOffset(index, total) {
  if (total <= 1) return { dx: 0, dy: 0 };
  const angle = (index / total) * Math.PI * 2;
  return {
    dx: Math.cos(angle) * STACK_OFFSET_PX,
    dy: Math.sin(angle) * STACK_OFFSET_PX,
  };
}

export default function MapCanvas({ token, orgId, members, myId }) {
  const canvasRef = useRef(null);
  const imagesRef = useRef(new Map()); // url -> HTMLImageElement
  const renderedRef = useRef({}); // user_id -> {x, y} (eased, float)
  const renderedOffsetRef = useRef({}); // user_id -> {dx, dy} (eased, float)
  const seenPlacementsRef = useRef({}); // user_id -> last placement drawn
  const walkRef = useRef({}); // user_id -> {facing, distance, movedAt}

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
        const dx = target.x - current.x;
        const dy = target.y - current.y;
        const dist = Math.hypot(dx, dy);
        const maxStep = USER_SPEED * dt;
        const next =
          dist <= maxStep
            ? { x: target.x, y: target.y }
            : {
                x: current.x + (dx / dist) * maxStep,
                y: current.y + (dy / dist) * maxStep,
              };
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

      const camera = rendered[myId] ?? { x: 0, y: 0 };
      const originPxX = w / 2 - camera.x * CELL_PX;
      const originPxY = h / 2 - camera.y * CELL_PX;

      // Grid lines, only across whatever range is actually on screen.
      const firstCol = Math.floor(-originPxX / CELL_PX);
      const lastCol = Math.ceil((w - originPxX) / CELL_PX);
      const firstRow = Math.floor(-originPxY / CELL_PX);
      const lastRow = Math.ceil((h - originPxY) / CELL_PX);

      ctx.strokeStyle = border;
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (
        let col = Math.max(0, firstCol);
        col <= Math.min(OFFICE_SIZE, lastCol);
        col++
      ) {
        const x = Math.round(originPxX + col * CELL_PX) + 0.5;
        ctx.moveTo(x, Math.max(0, originPxY));
        ctx.lineTo(x, Math.min(h, originPxY + OFFICE_SIZE * CELL_PX));
      }
      for (
        let row = Math.max(0, firstRow);
        row <= Math.min(OFFICE_SIZE, lastRow);
        row++
      ) {
        const y = Math.round(originPxY + row * CELL_PX) + 0.5;
        ctx.moveTo(Math.max(0, originPxX), y);
        ctx.lineTo(Math.min(w, originPxX + OFFICE_SIZE * CELL_PX), y);
      }
      ctx.stroke();

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
          const maxOffsetStep = STACK_EASE_PX_PER_SEC * dt;
          const { dx, dy } =
            odist <= maxOffsetStep
              ? target
              : {
                  dx: current.dx + (odx / odist) * maxOffsetStep,
                  dy: current.dy + (ody / odist) * maxOffsetStep,
                };
          renderedOffsets[member.id] = { dx, dy };
          const cx = originPxX + (pos.x + 0.5) * CELL_PX + dx;
          const cy = originPxY + (pos.y + 0.5) * CELL_PX + dy;

          // Snapped to whole device pixels so the (unsmoothed) sprite
          // doesn't shimmer as it eases between cells.
          const left = Math.round((cx - SPRITE_PX / 2) * ratio) / ratio;
          const top = Math.round((cy - SPRITE_PX / 2) * ratio) / ratio;

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
              ctx.translate(left + SPRITE_PX, top);
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
              SPRITE_PX,
              SPRITE_PX,
            );
            ctx.restore();
          }

          const ix = left + SPRITE_PX - USER_ICON_PX / 2;
          const iy = top + USER_ICON_PX / 2;
          const image = getImage(avatarUrl(member.picture));
          ctx.save();
          ctx.beginPath();
          ctx.arc(ix, iy, USER_ICON_PX / 2, 0, Math.PI * 2);
          ctx.closePath();
          if (image) {
            ctx.clip();
            ctx.drawImage(
              image,
              ix - USER_ICON_PX / 2,
              iy - USER_ICON_PX / 2,
              USER_ICON_PX,
              USER_ICON_PX,
            );
          } else {
            ctx.fillStyle =
              member.id === myId
                ? cssVar("--color-brand") || "#5b8def"
                : cssVar("--color-muted") || "#3a4763";
            ctx.fill();
            ctx.fillStyle = "#fff";
            ctx.font = "600 9px sans-serif";
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
    // Deliberately mount-only -- see `stateRef` above.
  }, []);

  const handleClick = (event) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const rendered = renderedRef.current;
    const camera = rendered[myId] ?? { x: 0, y: 0 };
    const originPxX = rect.width / 2 - camera.x * CELL_PX;
    const originPxY = rect.height / 2 - camera.y * CELL_PX;
    const px = event.clientX - rect.left;
    const py = event.clientY - rect.top;
    const x = Math.floor((px - originPxX) / CELL_PX);
    const y = Math.floor((py - originPxY) / CELL_PX);
    moveTo(x, y);
  };

  return (
    <canvas
      ref={canvasRef}
      onClick={handleClick}
      tabIndex={0}
      className="block size-full cursor-pointer outline-none"
    />
  );
}
