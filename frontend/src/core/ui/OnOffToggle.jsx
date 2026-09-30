//! On/Off segmented control, styled like `ThemeToggle`.

export default function OnOffToggle({ value, onChange }) {
  return (
    <div className="inline-flex rounded-card border p-0.5">
      {[
        [true, "On"],
        [false, "Off"],
      ].map(([option, label]) => (
        <button
          key={label}
          onClick={() => onChange(option)}
          className={`rounded-[calc(var(--radius-card)-0.2rem)] px-2.5 py-1 text-xs font-medium ${
            value === option
              ? "bg-brand text-on-brand"
              : "text-ink-muted hover:text-ink"
          }`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
