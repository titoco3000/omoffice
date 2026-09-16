//! A circular picture, or a first-letter placeholder when there is none --
//! the shared look for a user avatar or org icon everywhere one shows up.

export default function Avatar({ url, name, size = "size-8", className = "" }) {
  if (url) {
    return (
      <img
        src={url}
        alt=""
        className={`${size} shrink-0 rounded-full object-cover ${className}`}
      />
    );
  }
  return (
    <span
      className={`flex ${size} shrink-0 items-center justify-center rounded-full bg-brand text-sm font-semibold text-on-brand ${className}`}
    >
      {name?.trim()?.charAt(0).toUpperCase() ?? "?"}
    </span>
  );
}
