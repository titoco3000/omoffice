//! The org's map and its editor, shared by everything that shows or edits
//! it: `MapCanvas` (draws it, paints on it), the sidebar / mobile roster
//! panel (the edit button) and `MapRelocationProvider` (needs the walls).
//! Lives above all of them (see `app/App.jsx`) so one copy of the map, and
//! one edit session, is what they all see.
//!
//! The map is `map.png`'s pixels (see `tileMap.js`), loaded from the
//! backend's `maps/<org-id>.png` (`getOrgMap`) or, for an org that never
//! saved one (404), the bundled default. Two copies are kept: `saved`, what
//! the server holds, and `draft`, what the editor paints on and what
//! everything draws. Any difference between them is `dirty`, which is what
//! offers the Save button.
//!
//! Saving goes through the user's staging upload (`requestUploadUrl`,
//! `uploadToPresignedUrl`), then `saveOrgMap` asks the backend to promote it
//! -- which broadcasts `MAP_UPDATED_CHANNEL`, so every tab (this one too)
//! fetches the new map. An unsaved draft survives that: only a draft with no
//! edits of its own is replaced by what arrives.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { ApiError } from "../../core/api";
import { useAuth } from "../../core/auth/AuthContext";
import { messenger } from "../../core/messenger";
import { requestUploadUrl, uploadToPresignedUrl } from "../avatars";
import { useOrgSession } from "../org";
import { MAP_UPDATED_CHANNEL, getOrgMap, saveOrgMap } from "./api";
import {
  LEVELS,
  clonePixels,
  loadDefaultPixels,
  paintCell,
  pixelsFromBlob,
  pixelsToBlob,
  preloadSprites,
  samePixels,
  tileMapFromPixels,
} from "./tileMap";
import { tilesOnLevel } from "./tiles";

const MapEditorContext = createContext(null);

// The org's saved map, or the default when it has none. `failed` marks a
// fetch that went wrong for any other reason: the default is shown, but
// saving would then overwrite a map that was never seen.
async function loadOrgPixels(token, orgId) {
  try {
    return { pixels: await pixelsFromBlob(await getOrgMap(token, orgId)) };
  } catch (error) {
    const missing = error instanceof ApiError && error.status === 404;
    return { pixels: await loadDefaultPixels(), failed: !missing };
  }
}

// Cells on the straight line from `a` to `b`, both included.
function lineCells(a, b) {
  const cells = [];
  const steps = Math.max(Math.abs(b.x - a.x), Math.abs(b.y - a.y), 1);
  for (let i = 0; i <= steps; i++) {
    cells.push({
      x: Math.round(a.x + ((b.x - a.x) * i) / steps),
      y: Math.round(a.y + ((b.y - a.y) * i) / steps),
    });
  }
  return cells;
}

export function MapEditorProvider({ children }) {
  const { token } = useAuth();
  const { currentOrg } = useOrgSession();
  const orgId = currentOrg?.id;

  // `{orgId, saved, draft, failed}` for the org last loaded; ignored while
  // it isn't the current one, so switching orgs never shows the old map.
  const [loaded, setLoaded] = useState(null);
  const [editingOrg, setEditingOrg] = useState(null);
  const [tool, setTool] = useState({ level: LEVELS[0], tile: null });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(null);

  const maps = loaded?.orgId === orgId ? loaded : null;
  const latestRef = useRef({});
  useEffect(() => {
    latestRef.current = { token, orgId, tool, maps };
  });

  const editing = Boolean(orgId) && editingOrg === orgId;
  const setEditing = useCallback(
    (on) => setEditingOrg(on ? latestRef.current.orgId : null),
    [],
  );

  useEffect(() => {
    if (!orgId) return;
    let cancelled = false;
    (async () => {
      await preloadSprites();
      const { pixels, failed } = await loadOrgPixels(
        latestRef.current.token,
        orgId,
      );
      if (cancelled) return;
      setLoaded({
        orgId,
        saved: pixels,
        draft: clonePixels(pixels),
        failed: Boolean(failed),
      });
    })().catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [orgId]);

  // Someone (maybe this tab) saved a new map: take it, unless there are
  // unsaved edits here that it would wipe out.
  useEffect(
    () =>
      messenger.on(MAP_UPDATED_CHANNEL, async (message) => {
        const { token, orgId } = latestRef.current;
        if (!orgId || message?.org_id !== orgId) return;
        const { pixels, failed } = await loadOrgPixels(token, orgId);
        if (failed) return;
        setLoaded((prev) =>
          prev?.orgId !== orgId
            ? prev
            : {
                ...prev,
                saved: pixels,
                draft: samePixels(prev.draft, prev.saved)
                  ? clonePixels(pixels)
                  : prev.draft,
                failed: false,
              },
        );
      }),
    [],
  );

  const draft = maps?.draft;
  const tileMap = useMemo(
    () => (draft ? tileMapFromPixels(draft) : null),
    [draft],
  );
  const dirty = useMemo(
    () => (maps ? !samePixels(maps.draft, maps.saved) : false),
    [maps],
  );

  // Paints the current tool along the line `from` -> `to` (cells `{x, y}`).
  const paint = useCallback((from, to) => {
    const { level, tile } = latestRef.current.tool;
    const forOrg = latestRef.current.orgId;
    setLoaded((prev) => {
      if (prev?.orgId !== forOrg) return prev;
      const next = clonePixels(prev.draft);
      let changed = false;
      for (const cell of lineCells(from, to)) {
        changed = paintCell(next, level, cell.x, cell.y, tile) || changed;
      }
      return changed ? { ...prev, draft: next } : prev;
    });
  }, []);

  const discard = useCallback(() => {
    setError(null);
    setLoaded((prev) =>
      prev ? { ...prev, draft: clonePixels(prev.saved) } : prev,
    );
  }, []);

  const save = useCallback(async () => {
    const { token, orgId, maps } = latestRef.current;
    if (!maps || !orgId) return;
    const snapshot = clonePixels(maps.draft);
    setSaving(true);
    setError(null);
    try {
      const { upload_url: uploadUrl } = await requestUploadUrl(token);
      await uploadToPresignedUrl(uploadUrl, await pixelsToBlob(snapshot));
      await saveOrgMap(token, orgId);
      setLoaded((prev) =>
        prev?.orgId === orgId ? { ...prev, saved: snapshot } : prev,
      );
    } catch (err) {
      setError(err.message || "Could not save the map");
    } finally {
      setSaving(false);
    }
  }, []);

  const selectTool = useCallback((level, tile) => setTool({ level, tile }), []);
  const selectLevel = useCallback(
    (level) => setTool({ level, tile: tilesOnLevel(level)[0] ?? null }),
    [],
  );

  const value = {
    tileMap,
    // Anyone in the org may edit, once its map has loaded intact.
    canEdit: Boolean(tileMap) && !maps.failed,
    editing,
    setEditing,
    tool,
    selectTool,
    selectLevel,
    paint,
    dirty,
    saving,
    error,
    save,
    discard,
  };
  return (
    <MapEditorContext.Provider value={value}>
      {children}
    </MapEditorContext.Provider>
  );
}

export const useMapEditor = () => useContext(MapEditorContext);

/// The org's decoded map (with any unsaved edits), `null` until it has
/// loaded.
export const useTileMap = () => useContext(MapEditorContext).tileMap;
