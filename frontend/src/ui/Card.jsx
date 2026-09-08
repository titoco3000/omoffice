/// A panel on the page background. `Card.Header` is the title row every
/// section shares, so headings never drift in size between screens.
export default function Card({ className = "", children, ...rest }) {
  return (
    <section
      className={`rounded-card border border-line bg-surface p-4 ${className}`}
      {...rest}
    >
      {children}
    </section>
  );
}

export function CardHeader({ title, subtitle, actions }) {
  return (
    <div className="mb-3 flex items-start gap-3">
      <div className="min-w-0 flex-1">
        <h2 className="truncate text-sm font-semibold text-ink">{title}</h2>
        {subtitle && (
          <p className="mt-0.5 truncate text-xs text-ink-muted">{subtitle}</p>
        )}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  );
}
