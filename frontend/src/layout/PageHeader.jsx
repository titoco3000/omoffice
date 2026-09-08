/// The bar every page inside `NavContainer` starts with: a title on the
/// left, whatever that page can do on the right. It is the only thing
/// keeping page titles the same size as each other.
export default function PageHeader({ title, subtitle, actions }) {
  return (
    <header className="flex shrink-0 items-center gap-3 border-b border-line bg-surface px-4 py-3">
      <div className="min-w-0 flex-1">
        <h1 className="truncate text-base font-semibold text-ink">{title}</h1>
        {subtitle && <p className="truncate text-xs text-ink-muted">{subtitle}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}
