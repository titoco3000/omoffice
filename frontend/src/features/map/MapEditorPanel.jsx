import { FiCheck, FiRotateCcw, FiSave, FiSlash } from "react-icons/fi";
import AsyncButton from "../../core/ui/AsyncButton";
import { useMapEditor } from "./MapEditorContext";
import { LEVELS } from "./tileMap";
import { tilePreview, tilesOnLevel } from "./tiles";

const TABS = { base: "Floor", mid: "Furniture", top: "Objects" };

// The edit-mode toolbox: a tab per level, that level's tiles, an eraser,
// and -- once the map differs from what the server holds -- Save.
export default function MapEditorPanel() {
  const {
    tool,
    selectTool,
    selectLevel,
    dirty,
    saving,
    error,
    save,
    discard,
    setEditing,
  } = useMapEditor();

  return (
    <div className="absolute left-4 top-4 flex max-w-[calc(100%-6rem)] flex-col gap-2 rounded-card border bg-surface p-2 shadow-lg">
      <div className="flex gap-1">
        {LEVELS.map((level) => (
          <button
            key={level}
            onClick={() => selectLevel(level)}
            className={`rounded-card px-2.5 py-1 text-xs font-medium ${
              tool.level === level
                ? "bg-brand text-on-brand"
                : "text-ink-muted hover:text-ink"
            }`}
          >
            {TABS[level]}
          </button>
        ))}
      </div>

      <div className="flex flex-wrap gap-1">
        {tilesOnLevel(tool.level).map((name) => {
          const { url } = tilePreview(name);
          return (
            <button
              key={name}
              onClick={() => selectTool(tool.level, name)}
              title={name}
              className={`flex size-10 items-center justify-center rounded-card border ${
                tool.tile === name ? "border-brand bg-muted" : "hover:bg-muted"
              }`}
            >
              <img
                src={url}
                alt={name}
                className="size-8 object-contain [image-rendering:pixelated]"
              />
            </button>
          );
        })}
        <button
          onClick={() => selectTool(tool.level, null)}
          title="Eraser"
          className={`flex size-10 items-center justify-center rounded-card border text-ink-muted ${
            tool.tile === null ? "border-brand bg-muted" : "hover:bg-muted"
          }`}
        >
          <FiSlash />
        </button>
      </div>

      {error && <p className="text-xs text-danger">{error}</p>}

      <div className="flex items-center gap-1">
        {dirty && (
          <>
            <AsyncButton
              onClick={save}
              disabled={saving}
              className="flex items-center gap-1 rounded-card bg-brand px-2.5 py-1 text-xs font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
            >
              <FiSave /> Save
            </AsyncButton>
            <button
              onClick={discard}
              disabled={saving}
              title="Discard changes"
              className="flex items-center gap-1 rounded-card px-2.5 py-1 text-xs font-medium text-ink-muted hover:text-ink disabled:opacity-50"
            >
              <FiRotateCcw /> Discard
            </button>
          </>
        )}
        <button
          onClick={() => setEditing(false)}
          className="ml-auto flex items-center gap-1 rounded-card px-2.5 py-1 text-xs font-medium text-ink-muted hover:text-ink"
        >
          <FiCheck /> Done
        </button>
      </div>
    </div>
  );
}
