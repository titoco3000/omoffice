import { createContext, useContext, useEffect, useRef, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { CHANNELS, Messenger } from "./messenger";

export { CHANNELS };

const MessengerContext = createContext(null);

/**
 * Owns the one `Messenger` a session has, opened with the current token and
 * closed when it changes. Everything below just names channels.
 */
export function MessengerProvider({ children }) {
  const { token, isAuthenticated } = useAuth();
  const activeToken = isAuthenticated ? token : null;
  const [session, setSession] = useState({ token: null, messenger: null });
  const [status, setStatus] = useState("closed");

  // Adjusted during render (react.dev's pattern, same as `OrgProvider`) so
  // the first render below already has the session's messenger rather than
  // a null one children have to guard against for a frame.
  if (session.token !== activeToken) {
    setSession({
      token: activeToken,
      messenger: activeToken ? new Messenger(activeToken) : null,
    });
    setStatus(activeToken ? "connecting" : "closed");
  }

  const { messenger } = session;

  useEffect(() => {
    if (!messenger) return undefined;
    const unsubscribe = messenger.onStatus(setStatus);
    messenger.connect();
    return () => {
      unsubscribe();
      messenger.close();
    };
  }, [messenger]);

  return (
    <MessengerContext.Provider value={{ messenger, status }}>
      {children}
    </MessengerContext.Provider>
  );
}

export function useMessenger() {
  const context = useContext(MessengerContext);
  if (!context) throw new Error("useMessenger must be used inside a MessengerProvider");
  return context;
}

/**
 * Subscribe to one channel for as long as the component is mounted. The
 * handler is held in a ref, so a new closure each render does not
 * resubscribe (and does not need memoizing at the call site).
 */
export function useChannel(channel, handler) {
  const { messenger } = useMessenger();
  const handlerRef = useRef(handler);
  useEffect(() => {
    handlerRef.current = handler;
  });

  useEffect(() => {
    if (!messenger) return undefined;
    return messenger.on_message(channel, (message) => handlerRef.current(message));
  }, [messenger, channel]);
}

/**
 * Report part of this tab's context (`{org}`, `{meeting}`) for as long as
 * the component is mounted, and clear it on the way out.
 */
export function usePresence(patch) {
  const { messenger } = useMessenger();
  const serialized = JSON.stringify(patch);

  useEffect(() => {
    if (!messenger) return undefined;
    const reported = JSON.parse(serialized);
    messenger.setContext(reported);
    return () => {
      const cleared = Object.fromEntries(Object.keys(reported).map((key) => [key, null]));
      messenger.setContext(cleared);
    };
  }, [messenger, serialized]);
}
