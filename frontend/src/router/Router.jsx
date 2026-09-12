//! A path-based router with no dependency: the History API plus a context
//! for the current path. Enough for a handful of top-level pages.

import { createContext, useContext, useEffect, useState } from "react";

const LocationContext = createContext("/");

export function navigate(path) {
  window.history.pushState({}, "", path);
  window.dispatchEvent(new PopStateEvent("popstate"));
}

export function RouterProvider({ children }) {
  const [path, setPath] = useState(window.location.pathname);

  useEffect(() => {
    const onPopState = () => setPath(window.location.pathname);
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  return <LocationContext.Provider value={path}>{children}</LocationContext.Provider>;
}

export function useLocation() {
  return useContext(LocationContext);
}

export function Link({ to, className, children }) {
  return (
    <a
      href={to}
      className={className}
      onClick={(event) => {
        event.preventDefault();
        navigate(to);
      }}
    >
      {children}
    </a>
  );
}
