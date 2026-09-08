/// The one way a failed request is shown: `describeFailure`'s single line,
/// in the theme's danger color. `tone="info"` for a neutral message.
export default function Notice({ tone = "danger", className = "", children }) {
  if (!children) return null;
  return (
    <p
      className={`rounded-lg border px-3 py-2 font-mono text-xs break-all ${
        tone === "danger"
          ? "border-danger/40 bg-danger/10 text-danger"
          : "border-line bg-muted text-ink-muted"
      } ${className}`}
    >
      {children}
    </p>
  );
}
