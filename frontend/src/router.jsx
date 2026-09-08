import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";

/**
 * The whole router: five routes, the History API, and no dependency.
 *
 * The bundle is already over vite's warning because of the Chime SDK
 * (CLAUDE.md), and this needs exactly three things a library would bring —
 * a current path, `navigate`, and `/meeting/:code` pattern matching. Deep
 * links work in production because CloudFront already rewrites 403/404 to
 * `/index.html` (`infra/frontend.tf`).
 */
const RouterContext = createContext(null);

export function RouterProvider({ children }) {
  const [path, setPath] = useState(() => window.location.pathname);

  useEffect(() => {
    const onPopState = () => setPath(window.location.pathname);
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  const navigate = useCallback((to, { replace = false } = {}) => {
    if (to === window.location.pathname) return;
    window.history[replace ? "replaceState" : "pushState"]({}, "", to);
    setPath(to);
  }, []);

  const value = useMemo(() => ({ path, navigate }), [path, navigate]);
  return <RouterContext.Provider value={value}>{children}</RouterContext.Provider>;
}

export function useRouter() {
  const context = useContext(RouterContext);
  if (!context) throw new Error("useRouter must be used inside a RouterProvider");
  return context;
}

export function useNavigate() {
  return useRouter().navigate;
}

/// The current path, e.g. `/meeting/abc`.
export function usePath() {
  return useRouter().path;
}

/**
 * `matchPath("/meeting/:code", "/meeting/abc")` -> `{code: "abc"}`, and
 * `null` when the shape doesn't match. Segment counts must be equal — there
 * are no wildcards and nothing needs them.
 */
export function matchPath(pattern, path) {
  const wanted = pattern.split("/").filter(Boolean);
  const actual = path.split("/").filter(Boolean);
  if (wanted.length !== actual.length) return null;

  const params = {};
  for (let index = 0; index < wanted.length; index += 1) {
    const segment = wanted[index];
    if (segment.startsWith(":")) {
      params[segment.slice(1)] = decodeURIComponent(actual[index]);
    } else if (segment !== actual[index]) {
      return null;
    }
  }
  return params;
}

/// An `<a>` that navigates in-page, while still being a real link a person
/// can middle-click or copy.
export function Link({ to, onClick, children, ...rest }) {
  const navigate = useNavigate();

  function handleClick(event) {
    onClick?.(event);
    if (event.defaultPrevented) return;
    // Leave the modified clicks to the browser: new tab, new window, save.
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    if (event.button !== 0) return;
    event.preventDefault();
    navigate(to);
  }

  return (
    <a href={to} onClick={handleClick} {...rest}>
      {children}
    </a>
  );
}

/// Renders nothing and replaces the current entry — a guard's "you are not
/// allowed here, go there instead".
export function Redirect({ to }) {
  const navigate = useNavigate();
  useEffect(() => {
    navigate(to, { replace: true });
  }, [navigate, to]);
  return null;
}
