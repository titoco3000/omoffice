const TONES = {
  neutral: "bg-muted text-ink-muted",
  brand: "bg-brand/15 text-brand",
  success: "bg-success/15 text-success",
  warning: "bg-warning/20 text-warning",
  danger: "bg-danger/15 text-danger",
};

/// A small status word: a role, a stage, "live".
export default function Badge({ tone = "neutral", className = "", children }) {
  return (
    <span
      className={`inline-flex items-center rounded px-1.5 py-0.5 text-xs whitespace-nowrap ${TONES[tone]} ${className}`}
    >
      {children}
    </span>
  );
}
