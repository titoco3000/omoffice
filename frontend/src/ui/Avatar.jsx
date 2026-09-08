/// A person, as their first letter. There are no uploaded pictures yet, so
/// the whole identity is the email the token was issued for.
export default function Avatar({
  email,
  size = 'md',
  online = null,
  className = '',
}) {
  const sizes = {
    sm: 'h-7 w-7 text-[11px]',
    md: 'h-9 w-9 text-sm',
    lg: 'h-14 w-14 text-xl',
  };
  const letter =
    String(email ?? '?')
      .trim()
      .charAt(0)
      .toUpperCase() || '?';

  return (
    <span className={`relative inline-flex shrink-0 ${className}`}>
      <span
        className={`inline-flex items-center justify-center rounded-full bg-brand/15 font-semibold text-brand ${sizes[size]}`}
        title={email ?? undefined}
      >
        {letter}
      </span>
      {online !== null && (
        <span
          className={`absolute right-0 bottom-0 h-2.5 w-2.5 rounded-full ${
            online
              ? 'border-2 border-surface bg-success'
              : 'border-2 border-ink-faint bg-surface'
          }`}
          aria-hidden='true'
        />
      )}
    </span>
  );
}
