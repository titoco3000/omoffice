import { THEME_MODES, useTheme } from "../theme/ThemeContext";
import { IconMonitor, IconMoon, IconSun } from "../ui/icons";

const ICONS = {
  system: IconMonitor,
  light: IconSun,
  dark: IconMoon,
};

/// The theme's three states as three buttons. The whole of the app's
/// appearance settings, used on `/` and in the profile's settings panel.
export default function ThemeToggle() {
  const { mode, setMode } = useTheme();

  return (
    <div className="inline-flex items-center gap-0.5 rounded-lg border border-line bg-surface p-0.5">
      {THEME_MODES.map((option) => {
        const Icon = ICONS[option];
        return (
          <button
            key={option}
            type="button"
            onClick={() => setMode(option)}
            title={option}
            aria-label={`${option} theme`}
            aria-pressed={mode === option}
            className={`rounded-md p-1.5 transition-colors ${
              mode === option
                ? "bg-brand text-on-brand"
                : "text-ink-muted hover:bg-muted hover:text-ink"
            }`}
          >
            <Icon className="h-4 w-4" />
          </button>
        );
      })}
    </div>
  );
}
