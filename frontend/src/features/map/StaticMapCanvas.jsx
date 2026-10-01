//! A small, read-only picture of an org's whole map: terrain plus one dot per
//! member at their stored cell (bright when online). For someone looking at
//! an org they aren't in -- the sys-admin panel -- so it has no session: no
//! movement, no live updates, no websocket. Fetched once; the backend lets a
//! sys-admin read the positions and map of any org.

import { useEffect, useRef, useState } from "react";
import { ApiError } from "../../core/api";
import { OFFICE_SIZE, getOrgMap, listMapPositions } from "./api";
import { createTileRenderer } from "./tiles";
import {
  loadDefaultPixels,
  pixelsFromBlob,
  preloadSprites,
  tileMapFromPixels,
} from "./tileMap";

function cssVar(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name);
}

// The org's saved map, or the bundled default for one that never saved one.
async function loadTileMap(token, orgId) {
  await preloadSprites();
  try {
    return tileMapFromPixels(
      await pixelsFromBlob(await getOrgMap(token, orgId)),
    );
  } catch (error) {
    if (!(error instanceof ApiError) || error.status !== 404) throw error;
    return tileMapFromPixels(await loadDefaultPixels());
  }
}

export default function StaticMapCanvas({ token, orgId, members }) {
  const canvasRef = useRef(null);
  const [tileMap, setTileMap] = useState(null);
  const [positions, setPositions] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([loadTileMap(token, orgId), listMapPositions(token, orgId)])
      .then(([map, data]) => {
        if (cancelled) return;
        setTileMap(map);
        setPositions(data.positions);
      })
      .catch((err) => !cancelled && setError(err.message));
    return () => {
      cancelled = true;
    };
  }, [token, orgId]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !tileMap || !positions) return;

    const images = new Map();
    let raf;
    const draw = () => {
      const ratio = window.devicePixelRatio || 1;
      canvas.width = canvas.clientWidth * ratio;
      canvas.height = canvas.clientHeight * ratio;
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      const ctx = canvas.getContext("2d");
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
      ctx.imageSmoothingEnabled = false;
      ctx.fillStyle = cssVar("--color-bg") || "#0b0f19";
      ctx.fillRect(0, 0, w, h);

      // The whole map, fitted and centered.
      const cellPx = Math.min(w, h) / OFFICE_SIZE;
      const originX = (w - cellPx * OFFICE_SIZE) / 2;
      const originY = (h - cellPx * OFFICE_SIZE) / 2;

      let pending = false;
      const getImage = (url) => {
        if (!url) return null;
        let img = images.get(url);
        if (!img) {
          img = new Image();
          img.onload = () => {
            if (!pending) pending = true;
            cancelAnimationFrame(raf);
            raf = requestAnimationFrame(draw);
          };
          img.src = url;
          images.set(url, img);
        }
        return img.complete && img.naturalWidth > 0 ? img : null;
      };
      const tiles = createTileRenderer(getImage);
      const edge = (origin, index) =>
        Math.round((origin + index * cellPx) * ratio) / ratio;
      for (const pass of [
        tiles.drawBase,
        tiles.drawTransitions,
        tiles.drawMid,
        tiles.drawTop,
      ]) {
        for (let row = 0; row < tileMap.height; row++) {
          for (let col = 0; col < tileMap.width; col++) {
            pass(
              ctx,
              tileMap,
              col,
              row,
              edge(originX, col),
              edge(originY, row),
              edge(originX, col + 1),
              edge(originY, row + 1),
            );
          }
        }
      }

      const online = new Set(
        members.filter((member) => member.online).map((member) => member.id),
      );
      const radius = Math.max(2.5, cellPx * 1.2);
      for (const position of positions) {
        const isOnline = online.has(position.user_id);
        ctx.globalAlpha = isOnline ? 1 : 0.45;
        ctx.fillStyle = isOnline
          ? cssVar("--color-brand") || "#5b8def"
          : cssVar("--color-ink-muted") || "#888";
        ctx.strokeStyle = cssVar("--color-surface") || "#fff";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.arc(
          originX + (position.x + 0.5) * cellPx,
          originY + (position.y + 0.5) * cellPx,
          radius,
          0,
          Math.PI * 2,
        );
        ctx.fill();
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    };

    draw();
    const observer = new ResizeObserver(draw);
    observer.observe(canvas);
    return () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
    };
  }, [tileMap, positions, members]);

  if (error) return <p className="text-sm text-danger">{error}</p>;
  return (
    <canvas
      ref={canvasRef}
      className="aspect-square w-full max-w-md rounded-card border bg-bg"
    />
  );
}
