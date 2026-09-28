//! One `<canvas>` overlay for a content-share tile's shared board -- see
//! `features::meetings::board` (backend/lib/features/meetings/board.rs).
//! Strokes are freehand paths, normalized to the shared screen's own
//! rendered rect inside the tile (not the tile container -- see the
//! `recompute` effect below, `object-fit: contain` means the video is
//! letterboxed), redrawn on every change from a plain
//! `Map<strokeId, stroke>`. Ephemeral: nothing is persisted anywhere, this
//! component's own state is the only copy, gone the moment the tile
//! unmounts (the share ending) same as the tile itself.
//!
//! `ownerId` is the presenter's user id, not this tile's Chime `tileId` --
//! the id `board.rs` routes every message by, since a Chime tile id is
//! local to each client's own session and a meeting can have two
//! simultaneous shares. `ownerId === myId` is this tile's own presenter:
//! their instance is what answers a late joiner's `request_state` with its
//! current strokes; everyone else's instance sends that request once, on
//! mount.

import { useEffect, useRef, useState } from "react";
import { FiEdit2, FiTrash2 } from "react-icons/fi";
import { messenger } from "../../../core/messenger";
import { useIsTouch } from "../../../core/hooks/useViewport";
import { MEETING_BOARD_CHANNEL as CHANNEL } from "../api";

const COLORS = ["#ef4444", "#facc15", "#22c55e", "#3b82f6", "#f5f5f5"];
const STROKE_WIDTH = 3;
// Minimum distance (in the tile's displayed css px) between two points kept
// in a stroke -- what keeps a long stroke from being hundreds of points,
// same effect as simplifying before sending since points below this are
// never added at all.
const MIN_POINT_DISTANCE = 2;
// Point-to-segment hit distance for the eraser, in the tile's displayed css
// px (~6-8px at 1x per the design).
const ERASE_THRESHOLD = 8;

function drawStroke(ctx, stroke, rect) {
  if (stroke.points.length < 1) return;
  ctx.strokeStyle = stroke.color;
  ctx.lineWidth = stroke.width;
  ctx.beginPath();
  stroke.points.forEach(([nx, ny], i) => {
    const x = nx * rect.width;
    const y = ny * rect.height;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  });
  ctx.stroke();
}

function distanceToSegment(p, a, b, rect) {
  const px = p[0] * rect.width;
  const py = p[1] * rect.height;
  const ax = a[0] * rect.width;
  const ay = a[1] * rect.height;
  const bx = b[0] * rect.width;
  const by = b[1] * rect.height;
  const dx = bx - ax;
  const dy = by - ay;
  const lengthSquared = dx * dx + dy * dy;
  let t = lengthSquared === 0 ? 0 : ((px - ax) * dx + (py - ay) * dy) / lengthSquared;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

export default function BoardCanvas({ meetingId, ownerId, myId, videoRef }) {
  const isOwner = ownerId === myId;
  const isTouch = useIsTouch();
  const wrapperRef = useRef(null);
  const canvasRef = useRef(null);
  // The shared screen's own rendered rect inside this overlay, in css px --
  // `null` until the video reports real dimensions.
  const [rect, setRect] = useState(null);
  const [strokes, setStrokes] = useState(new Map());
  const strokesRef = useRef(strokes);
  // `null` -- drawing off, only the pen icon shows. `"pen"`/`"eraser"` --
  // the palette is open on that tool; picking the *same* one again (see
  // `selectPen`/`selectEraser`) is what turns it back off.
  const [tool, setTool] = useState(null);
  const [color, setColor] = useState(COLORS[0]);
  const paletteOpen = tool !== null;
  const selectPen = () => setTool((t) => (t === "pen" ? null : "pen"));
  const selectEraser = () => setTool((t) => (t === "eraser" ? null : "eraser"));
  // The in-flight stroke (or `{erasing: true}`) between pointerdown and
  // pointerup -- a ref, not state, since it's mutated on every pointermove
  // and never needs to trigger a render itself (the live draw happens
  // straight on the canvas).
  const drawingRef = useRef(null);
  const requestedRef = useRef(false);

  useEffect(() => {
    strokesRef.current = strokes;
  }, [strokes]);

  useEffect(() => {
    const video = videoRef.current;
    const wrapper = wrapperRef.current;
    if (!video || !wrapper) return;

    const recompute = () => {
      const cw = wrapper.clientWidth;
      const ch = wrapper.clientHeight;
      const vw = video.videoWidth;
      const vh = video.videoHeight;
      if (!cw || !ch || !vw || !vh) return;
      const scale = Math.min(cw / vw, ch / vh);
      const width = vw * scale;
      const height = vh * scale;
      setRect({ left: (cw - width) / 2, top: (ch - height) / 2, width, height });
    };

    recompute();
    video.addEventListener("loadedmetadata", recompute);
    const observer = new ResizeObserver(recompute);
    observer.observe(wrapper);
    return () => {
      video.removeEventListener("loadedmetadata", recompute);
      observer.disconnect();
    };
  }, [videoRef]);

  // Redraw the whole canvas from `strokes` -- simplest thing, fine at this
  // scale (per-tile stroke counts are small, freehand paths only).
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !rect) return;
    canvas.width = rect.width;
    canvas.height = rect.height;
    const ctx = canvas.getContext("2d");
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    for (const stroke of strokes.values()) drawStroke(ctx, stroke, rect);
  }, [strokes, rect]);

  const applyAdd = (stroke) =>
    setStrokes((prev) => new Map(prev).set(stroke.id, stroke));
  const applyErase = (strokeId) =>
    setStrokes((prev) => {
      if (!prev.has(strokeId)) return prev;
      const next = new Map(prev);
      next.delete(strokeId);
      return next;
    });

  // Every peer's `add`/`erase`, plus (for the owner) catch-up requests, and
  // (for whoever just asked) the owner's reply -- see `board.rs`. Every
  // mounted `BoardCanvas` in this tab gets every frame on this one channel;
  // `meeting_id`/`owner_id` is how each instance tells its own tile's
  // traffic apart from any other simultaneous share's.
  useEffect(() => {
    return messenger.on(CHANNEL, (message) => {
      if (message.meeting_id !== meetingId || message.owner_id !== ownerId)
        return;
      if (message.type === "add") {
        applyAdd(message.stroke);
      } else if (message.type === "erase") {
        applyErase(message.stroke_id);
      } else if (message.type === "request_state") {
        if (!isOwner) return;
        messenger.send(CHANNEL, {
          type: "state",
          meeting_id: meetingId,
          owner_id: ownerId,
          to: message.for,
          strokes: Array.from(strokesRef.current.values()),
        });
      } else if (message.type === "state") {
        if (message.to !== myId) return;
        setStrokes(new Map(message.strokes.map((s) => [s.id, s])));
      }
    });
  }, [meetingId, ownerId, isOwner, myId]);

  // Ask the owner for whatever's already drawn -- once, on mount, and not
  // for the owner's own instance (nothing to catch up on there).
  useEffect(() => {
    if (isOwner || requestedRef.current) return;
    requestedRef.current = true;
    messenger.send(CHANNEL, {
      type: "request_state",
      meeting_id: meetingId,
      owner_id: ownerId,
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const pointFromEvent = (e) => {
    const box = canvasRef.current.getBoundingClientRect();
    return [(e.clientX - box.left) / rect.width, (e.clientY - box.top) / rect.height];
  };

  const pixelDistance = (a, b) =>
    Math.hypot((a[0] - b[0]) * rect.width, (a[1] - b[1]) * rect.height);

  const eraseAt = (point) => {
    for (const stroke of strokesRef.current.values()) {
      const pts = stroke.points;
      for (let i = 0; i < pts.length - 1; i++) {
        if (distanceToSegment(point, pts[i], pts[i + 1], rect) <= ERASE_THRESHOLD) {
          applyErase(stroke.id);
          messenger.send(CHANNEL, {
            type: "erase",
            meeting_id: meetingId,
            owner_id: ownerId,
            stroke_id: stroke.id,
          });
          return;
        }
      }
    }
  };

  const onPointerDown = (e) => {
    if (!rect || !tool) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    const point = pointFromEvent(e);
    if (tool === "eraser") {
      drawingRef.current = { erasing: true };
      eraseAt(point);
      return;
    }
    drawingRef.current = {
      id: crypto.randomUUID(),
      color,
      width: STROKE_WIDTH,
      points: [point],
    };
  };

  const onPointerMove = (e) => {
    if (!drawingRef.current || !rect) return;
    const point = pointFromEvent(e);
    if (drawingRef.current.erasing) {
      eraseAt(point);
      return;
    }
    const points = drawingRef.current.points;
    if (pixelDistance(points[points.length - 1], point) < MIN_POINT_DISTANCE) return;
    points.push(point);
    // Live local draw, no round trip for your own stroke -- painted
    // straight on top of whatever's already committed.
    drawStroke(canvasRef.current.getContext("2d"), drawingRef.current, rect);
  };

  const onPointerUp = () => {
    const drawing = drawingRef.current;
    drawingRef.current = null;
    if (!drawing || drawing.erasing || drawing.points.length < 2) return;
    applyAdd(drawing);
    messenger.send(CHANNEL, {
      type: "add",
      meeting_id: meetingId,
      owner_id: ownerId,
      stroke: drawing,
    });
  };

  return (
    <div ref={wrapperRef} className="absolute inset-0">
      {rect && (
        <canvas
          ref={canvasRef}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerLeave={onPointerUp}
          style={{
            position: "absolute",
            left: rect.left,
            top: rect.top,
            width: rect.width,
            height: rect.height,
            touchAction: "none",
            // Idle (no tool picked) -- let clicks fall through to whatever
            // is behind it (the pin/mute cluster, say) instead of eating
            // them for a stroke that was never going to start anyway.
            pointerEvents: tool ? "auto" : "none",
          }}
        />
      )}
      <div
        // `z-10` -- `MeetingTile`'s own pin/mute/camera-off overlay is a
        // later sibling in the DOM (paints above this whole component by
        // default, dark tint included), and while it's `pointer-events-none`
        // outside its own buttons (so a click here still reaches these
        // buttons), that alone doesn't stop its tint from visually washing
        // this toolbar out. An explicit z-index makes it paint on top
        // instead, clear of the dimming -- it doesn't overlap the pin/mute
        // cluster (that's centered, this is pinned to the corner), so there
        // is nothing here for it to cover in turn.
        className={`absolute left-1.5 top-1.5 z-10 flex items-center gap-1 rounded-card bg-surface/90 p-1 opacity-0 transition-opacity ${
          isTouch ? "group-focus-within:opacity-100" : "group-hover:opacity-100"
        }`}
      >
        <button
          onClick={selectPen}
          title="Draw"
          className={`flex items-center rounded p-1.5 text-xs ${
            tool === "pen" ? "bg-brand text-on-brand" : "text-ink hover:bg-muted"
          }`}
        >
          <FiEdit2 />
        </button>
        {paletteOpen && (
          <>
            <button
              onClick={selectEraser}
              title="Erase"
              className={`flex items-center rounded p-1.5 text-xs ${
                tool === "eraser" ? "bg-brand text-on-brand" : "text-ink hover:bg-muted"
              }`}
            >
              <FiTrash2 />
            </button>
            {tool === "pen" &&
              COLORS.map((c) => (
                <button
                  key={c}
                  onClick={() => setColor(c)}
                  title={c}
                  style={{ backgroundColor: c }}
                  className={`size-4 rounded-full border ${
                    color === c ? "border-ink" : "border-transparent"
                  }`}
                />
              ))}
          </>
        )}
      </div>
    </div>
  );
}
