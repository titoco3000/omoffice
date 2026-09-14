//! Mobile layout is a width breakpoint, not a user-agent sniff -- matches
//! Tailwind's own `sm` breakpoint so the same cutoff governs both.
import { useEffect, useState } from "react";

const MOBILE_QUERY = "(max-width: 639px)";
const PORTRAIT_QUERY = "(orientation: portrait)";

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
