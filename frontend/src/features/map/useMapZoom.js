//! Map zoom, as the size of one cell in CSS px: from `MIN_CELLS_ACROSS`
//! cells filling the canvas's width, out to the whole `OFFICE_SIZE` map
//! fitting in it (see `zoomLimits`).
//!
//! Driven by `MapCanvas`'s +/- buttons (`zoomBy`) and by whatever the
//! browser would otherwise treat as page zoom, taken over while the map is
//! mounted -- zooming the page around a canvas that already scales itself
//! only makes the sidebar and buttons bigger:
//! - ctrl/cmd + `+` / `-` / `0` (anywhere, outside a text field);
//! - ctrl + scroll over the canvas, which is also how Chrome and Firefox
//!   report a trackpad pinch;
//! - Safari's own trackpad pinch (`gesture*` events);
//! - a two-finger touch pinch on the canvas (the canvas sets
//!   `touch-action: none`, so the browser leaves those to us).
//!
//! Held in a ref, not state: `MapCanvas`'s draw loop reads it every frame,
//! and it can change on every wheel/pointer event. It's the zoom being
//! headed for, not the one drawn: `MapCanvas` eases toward it
//! (`easeCellPx`), so a button press or wheel notch glides there rather
//! than jumping.

import { useCallback, useEffect, useRef, useState } from "react";
import { OFFICE_SIZE } from "./api";

export const DEFAULT_CELL_PX = 48;
const MIN_CELLS_ACROSS = 5;
// Factor per +/- button press or keyboard shortcut.
export const ZOOM_STEP = 1.25;
// Per unit of wheel `deltaY` -- trackpad pinches send small deltas, a mouse
// notch ~100 (or a burst of events adding up to that), each capped at
// `MAX_WHEEL_DELTA` so one notch is ~0.9x.
const WHEEL_ZOOM_RATE = 0.002;
const MAX_WHEEL_DELTA = 50;
// Time constant of the eased zoom: ~63% of the way to the target after
// this long, ~95% after three times it.
const ZOOM_EASE_SEC = 0.08;

// The cell size range for a `width` x `height` canvas: `max` fits
// `MIN_CELLS_ACROSS` cells across, `min` fits the whole map both ways.
export function zoomLimits(width, height) {
  const max = width / MIN_CELLS_ACROSS;
  const min = Math.min(max, Math.min(width, height) / OFFICE_SIZE);
  return { min, max };
}

export function clampCellPx(cellPx, width, height) {
  const { min, max } = zoomLimits(width, height);
  return Math.max(min, Math.min(max, cellPx));
}

// One frame (`dt` seconds) of easing the drawn cell size toward `target`,
// in log space so zooming in and out feel the same speed.
export function easeCellPx(current, target, dt) {
  if (Math.abs(Math.log(target / current)) < 0.001) return target;
  return current * (target / current) ** (1 - Math.exp(-dt / ZOOM_EASE_SEC));
}

export function useMapZoom(canvasRef) {
  const cellPxRef = useRef(DEFAULT_CELL_PX);
  // Whether the zoom being headed for sits at either limit -- state, unlike
  // `cellPxRef`, since the +/- buttons render off it. Only set when it
  // actually flips, so a pinch's stream of changes doesn't re-render.
  const [atLimit, setAtLimit] = useState({ min: false, max: false });

  const refreshLimits = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const { min, max } = zoomLimits(canvas.clientWidth, canvas.clientHeight);
    // With a margin: repeated steps land a hair off the limit in float.
    const next = {
      min: cellPxRef.current <= min * 1.001,
      max: cellPxRef.current >= max * 0.999,
    };
    setAtLimit((prev) =>
      prev.min === next.min && prev.max === next.max ? prev : next,
    );
  }, [canvasRef]);

  // Clamped as it's set, not just when drawn, so zooming past a limit
  // doesn't bank extra zoom that the other direction has to unwind first.
  const setCellPx = useCallback(
    (cellPx) => {
      const canvas = canvasRef.current;
      cellPxRef.current = canvas
        ? clampCellPx(cellPx, canvas.clientWidth, canvas.clientHeight)
        : cellPx;
      refreshLimits();
    },
    [canvasRef, refreshLimits],
  );

  // A resize moves the limits themselves -- re-clamping the target against
  // them too, same as `MapCanvas` does for what it draws.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const observer = new ResizeObserver(() => setCellPx(cellPxRef.current));
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [canvasRef, setCellPx]);

  const zoomBy = useCallback(
    (factor) => setCellPx(cellPxRef.current * factor),
    [setCellPx],
  );

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const onKeyDown = (event) => {
      if (!(event.ctrlKey || event.metaKey)) return;
      if (["INPUT", "TEXTAREA"].includes(event.target.tagName)) return;
      if (event.key === "+" || event.key === "=") zoomBy(ZOOM_STEP);
      else if (event.key === "-" || event.key === "_") zoomBy(1 / ZOOM_STEP);
      else if (event.key === "0") setCellPx(DEFAULT_CELL_PX);
      else return;
      event.preventDefault();
    };

    const onWheel = (event) => {
      if (!event.ctrlKey) return;
      event.preventDefault();
      const delta = Math.max(
        -MAX_WHEEL_DELTA,
        Math.min(MAX_WHEEL_DELTA, event.deltaY),
      );
      zoomBy(Math.exp(-delta * WHEEL_ZOOM_RATE));
    };

    let gestureStartCellPx = null;
    const onGestureStart = (event) => {
      event.preventDefault();
      gestureStartCellPx = cellPxRef.current;
    };
    const onGestureChange = (event) => {
      event.preventDefault();
      if (gestureStartCellPx !== null)
        setCellPx(gestureStartCellPx * event.scale);
    };
    const onGestureEnd = (event) => {
      event.preventDefault();
      gestureStartCellPx = null;
    };

    const pointers = new Map(); // pointerId -> {x, y}, touch only
    let pinchDistance = null;
    const currentPinchDistance = () => {
      const [a, b] = [...pointers.values()];
      return Math.hypot(a.x - b.x, a.y - b.y);
    };
    const onPointerDown = (event) => {
      if (event.pointerType !== "touch") return;
      pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
      if (pointers.size === 2) pinchDistance = currentPinchDistance();
    };
    const onPointerMove = (event) => {
      if (!pointers.has(event.pointerId)) return;
      pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
      if (pointers.size !== 2 || !pinchDistance) return;
      const distance = currentPinchDistance();
      if (distance > 0) zoomBy(distance / pinchDistance);
      pinchDistance = distance;
    };
    const onPointerUp = (event) => {
      pointers.delete(event.pointerId);
      if (pointers.size < 2) pinchDistance = null;
    };

    window.addEventListener("keydown", onKeyDown);
    canvas.addEventListener("wheel", onWheel, { passive: false });
    canvas.addEventListener("gesturestart", onGestureStart);
    canvas.addEventListener("gesturechange", onGestureChange);
    canvas.addEventListener("gestureend", onGestureEnd);
    canvas.addEventListener("pointerdown", onPointerDown);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerup", onPointerUp);
    canvas.addEventListener("pointercancel", onPointerUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      canvas.removeEventListener("wheel", onWheel);
      canvas.removeEventListener("gesturestart", onGestureStart);
      canvas.removeEventListener("gesturechange", onGestureChange);
      canvas.removeEventListener("gestureend", onGestureEnd);
      canvas.removeEventListener("pointerdown", onPointerDown);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerup", onPointerUp);
      canvas.removeEventListener("pointercancel", onPointerUp);
    };
  }, [canvasRef, zoomBy, setCellPx]);

  return { cellPxRef, zoomBy, atLimit };
}
