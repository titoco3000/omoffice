import { useMapMode } from "./MapModeContext";

const OPTIONS = [
  { value: "map", label: "Map" },
  { value: "no-graphics", label: "No graphics" },
];

export default function MapModeToggle({ className = "" }) {
  const { mode, setMode } = useMapMode();

  return (
    <div className={`inline-flex rounded-card border p-0.5 ${className}`}>
      {OPTIONS.map((option) => (
        <button
          key={option.value}
          onClick={() => setMode(option.value)}
          className={`rounded-[calc(var(--radius-card)-0.2rem)] px-2.5 py-1 text-xs font-medium ${
            mode === option.value
              ? "bg-brand text-on-brand"
              : "text-ink-muted hover:text-ink"
          }`}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
