//! Small inline loading spinner -- for a button mid-request, sized off the
//! current text color/size so it drops into any button without extra props.

export default function Spinner({ className = "" }) {
  return (
    <span
      className={`inline-block size-3.5 shrink-0 animate-spin rounded-full border-2 border-current border-t-transparent align-[-2px] ${className}`}
      aria-hidden="true"
    />
  );
}
