import { createContext, useContext, useEffect, useState } from "react";
import { buildPath, callApi } from "../api";
import { decodeClaims, isExpired } from "./jwt";

const STORAGE_KEY = "omoffice_token";

const AuthContext = createContext(null);

function loadStoredToken() {
  let token;
  try {
    token = localStorage.getItem(STORAGE_KEY);
  } catch {
    return null; // private browsing, storage disabled, etc.
  }
  if (!token || isExpired(decodeClaims(token))) return null;
  return token;
}

/**
 * The one place a token lives: seeded from `localStorage` (validated against
 * its own `exp` so a stale one doesn't outlive the session it belonged to),
 * kept in sync there on every change. `login(email)` is `GET
 * /auth/request-key/{email}` — the API hands back a token with no further
 * proof of ownership, so this is the entire login flow. `logout()` just
 * forgets it; nothing to tell the server.
 */
export function AuthProvider({ children }) {
  const [token, setToken] = useState(loadStoredToken);

  useEffect(() => {
    try {
      if (token) localStorage.setItem(STORAGE_KEY, token);
      else localStorage.removeItem(STORAGE_KEY);
    } catch {
      // Nothing to do if storage is unavailable — the session just won't
      // survive a refresh.
    }
  }, [token]);

  async function login(email) {
    const response = await callApi({
      path: buildPath("/auth/request-key/{email}", { email }),
    });
    if (response.ok) setToken(response.data.token ?? "");
    return response;
  }

  // An invite's accept route (`/invites/org/{id}/accept`,
  // `/invites/meetings/{id}/accept`) already hands back a token exactly like
  // `/auth/request-key/{email}` does -- this just adopts it, without
  // re-deriving it through `login`. Used by the magic-link handler and by
  // accepting a pending invite from inside an already-logged-in session.
  function loginWithToken(token) {
    setToken(token ?? "");
  }

  function logout() {
    setToken("");
  }

  const claims = decodeClaims(token);
  const isAuthenticated = Boolean(token) && !isExpired(claims);

  return (
    <AuthContext.Provider
      value={{
        token,
        email: claims?.sub ?? null,
        isAuthenticated,
        login,
        loginWithToken,
        logout,
      }}
    >
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const context = useContext(AuthContext);
  if (!context) throw new Error("useAuth must be used inside an AuthProvider");
  return context;
}
