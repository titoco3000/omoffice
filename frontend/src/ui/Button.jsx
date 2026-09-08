/**
 * Every button in the app. The variants are the only place a control's
 * colors are spelled out, so "what a primary button looks like" is one edit
 * here rather than a class string repeated per screen.
 */
const VARIANTS = {
  primary: "bg-brand text-on-brand hover:bg-brand-strong",
  secondary: "bg-muted text-ink hover:bg-line",
  ghost: "text-ink-muted hover:bg-muted hover:text-ink",
  danger: "bg-danger text-on-danger hover:opacity-90",
  outline: "border border-line text-ink hover:bg-muted",
};

const SIZES = {
  sm: "h-8 px-3 text-xs",
  md: "h-9 px-4 text-sm",
  icon: "h-9 w-9",
  "icon-sm": "h-8 w-8",
};

export default function Button({
  variant = "primary",
  size = "md",
  className = "",
  type = "button",
  ...rest
}) {
  return (
    <button
      type={type}
      className={`inline-flex shrink-0 items-center justify-center gap-2 rounded-lg font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 ${VARIANTS[variant]} ${SIZES[size]} ${className}`}
      {...rest}
    />
  );
}
