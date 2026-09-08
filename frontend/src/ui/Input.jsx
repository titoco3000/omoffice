/// The one text input. `mono` is for the values that are really identifiers
/// — emails, meeting codes — which read better in the theme's mono face.
export default function Input({ mono = false, className = "", ...rest }) {
  return (
    <input
      className={`h-9 min-w-0 rounded-lg border border-line bg-surface px-2.5 text-sm text-ink placeholder:text-ink-faint focus:border-brand focus:outline-none disabled:opacity-50 ${
        mono ? "font-mono" : ""
      } ${className}`}
      {...rest}
    />
  );
}
