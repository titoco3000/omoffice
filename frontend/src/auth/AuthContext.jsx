//! Session token: read once from `localStorage`, or from the `?login=`
//! query param a magic-link email drops us on -- `core::auth::request_key`'s
//! own contract (backend/lib/core/auth/mod.rs).

import { createContext, useContext, useEffect, useState } from "react";
import { UNAUTHORIZED_EVENT } from "../lib/api";
import { messenger } from "../lib/messenger";
import { navigate } from "../router/Router";

const STORAGE_KEY = "omoffice_token";
const AuthContext = createContext(null);

function readStoredToken() {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function tokenFromLoginLink() {
  try {
    return new URL(window.location.href).searchParams.get("login");
  } catch {
    return null;
  }
}

export function AuthProvider({ children }) {
  const [token, setToken] = useState(
    () => tokenFromLoginLink() ?? readStoredToken(),
  );

  useEffect(() => {
    const fromLink = tokenFromLoginLink();
    if (!fromLink) return;
    localStorage.setItem(STORAGE_KEY, fromLink);
    // An invite's magic link carries both -- keep `invite` riding along so
    // `Panels` can still read it off the URL it lands on.
    const invite = new URL(window.location.href).searchParams.get("invite");
    navigate(
      invite ? `/panels?invite=${encodeURIComponent(invite)}` : "/panels",
    );
  }, []);

  const logout = (redirectTo = "/") => {
    localStorage.removeItem(STORAGE_KEY);
    messenger.disconnect();
    setToken(null);
    navigate(redirectTo);
  };

  // One messenger for the whole session, connected as soon as there's a
  // token to connect it with -- every panel just subscribes to `messenger`,
  // see `lib/messenger.js`.
  useEffect(() => {
    if (token) messenger.connect(token);
    else messenger.disconnect();
  }, [token]);

  // A 401 means the token itself is dead (missing/malformed/expired) --
  // drop it and send them back to log in again, instead of every panel
  // re-firing the same doomed request against a session that's already gone.
  useEffect(() => {
    const onUnauthorized = () => logout("/login");
    window.addEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
  }, []);

  return (
    <AuthContext.Provider value={{ token, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
