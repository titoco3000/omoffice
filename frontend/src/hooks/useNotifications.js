import { useEffect, useRef, useState } from "react";
import { notifyWsUrl } from "../api";

/// How long a toast stays before it dismisses itself.
const TOAST_LIFETIME_MS = 8000;

/**
 * The live half of an invite's delivery: the presence socket, turning every
 * `{"type": "invite", ...}` frame into a toast. No reconnect loop.
 */
export function useNotifications(token) {
  const [toasts, setToasts] = useState([]);
  const nextIdRef = useRef(0);

  useEffect(() => {
    if (!token) return undefined;
    const socket = new WebSocket(notifyWsUrl(token));

    socket.onmessage = (event) => {
      let parsed;
      try {
        parsed = JSON.parse(event.data);
      } catch {
        return; // not JSON -- ignore rather than crash the socket over one frame
      }
      if (typeof parsed?.message !== "string") return;

      // `kind`/`invite_id` are what let clicking the toast accept the
      // invite instead of just showing a message — see `Toast`'s `onAccept`.
      nextIdRef.current += 1;
      const id = nextIdRef.current;
      setToasts((current) => [
        ...current,
        { id, message: parsed.message, kind: parsed.kind, inviteId: parsed.invite_id },
      ]);
      setTimeout(() => {
        setToasts((current) => current.filter((toast) => toast.id !== id));
      }, TOAST_LIFETIME_MS);
    };

    return () => socket.close();
  }, [token]);

  function dismiss(id) {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }

  return { toasts, dismiss };
}
