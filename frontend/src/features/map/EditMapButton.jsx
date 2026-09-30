import { FiEdit2 } from "react-icons/fi";
import { useMapEditor } from "./MapEditorContext";

// Enters/leaves the map editor; nothing until the org's map has loaded.
// `compact` shows just the icon (the collapsed sidebar rail).
export default function EditMapButton({ compact = false, className = "" }) {
  const { canEdit, editing, setEditing } = useMapEditor();
  if (!canEdit) return null;
  return (
    <button
      onClick={() => setEditing(!editing)}
      title={editing ? "Stop editing map" : "Edit map"}
      className={`flex items-center gap-2 rounded-card px-2 py-1.5 text-sm ${
        editing
          ? "bg-brand text-on-brand"
          : "text-ink-muted hover:bg-muted hover:text-ink"
      } ${compact ? "justify-center" : ""} ${className}`}
    >
      <FiEdit2 />
      {!compact && <span>{editing ? "Editing map" : "Edit map"}</span>}
    </button>
  );
}
