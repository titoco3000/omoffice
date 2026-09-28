//! Mobile layout is a breakpoint, not a user-agent sniff -- matches
//! Tailwind's own `sm` breakpoint so the same cutoff governs both. Checked
//! against both dimensions (comma = OR in a media query list), not just
//! width: a phone rotated to landscape has plenty of width but is still a
//! phone, and a width-only check would hand it the desktop sidebar.
import { useEffect, useState } from "react";

const MOBILE_QUERY = "(max-width: 15cm), (max-height: 15cm)";
const PORTRAIT_QUERY = "(orientation: portrait)";
const TOUCH_QUERY = "(pointer: coarse)";

function useMediaQuery(query) {
  const [matches, setMatches] = useState(
    () => window.matchMedia(query).matches,
  );

  useEffect(() => {
    const media = window.matchMedia(query);
    const onChange = (event) => setMatches(event.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [query]);

  return matches;
}

export function useIsMobile() {
  return useMediaQuery(MOBILE_QUERY);
}

export function useIsPortrait() {
  return useMediaQuery(PORTRAIT_QUERY);
}

// Whether the primary pointer is coarse (touch) rather than fine (mouse/
// trackpad) -- a touch laptop with a mouse attached still reads `false`
// here, which is what matters for hover-vs-tap affordances: a mouse is
// present, so `:hover` still works.
export function useIsTouch() {
  return useMediaQuery(TOUCH_QUERY);
}
