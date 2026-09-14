import { useTheme } from "./ThemeContext";

const OPTIONS = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export default function ThemeToggle({ className = "" }) {
  const { theme, setTheme } = useTheme();

  return (
    <div className={`inline-flex rounded-card border p-0.5 ${className}`}>
      {OPTIONS.map((option) => (
        <button
          key={option.value}
          onClick={() => setTheme(option.value)}
          className={`rounded-[calc(var(--radius-card)-0.2rem)] px-2.5 py-1 text-xs font-medium ${
            theme === option.value
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
