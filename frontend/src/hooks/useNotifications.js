import { useEffect, useRef, useState } from "react";
import { notifyWsUrl } from "../api";

/// How long a toast stays before it dismisses itself.
const TOAST_LIFETIME_MS = 8000;

/**
 * The live half of an invite's delivery: opens the room-less presence
 * socket (`notifyWsUrl`) once while `token` is set, and turns every
 * `{"type": "invite", ...}` frame it receives into a toast. Same bare-bones
 * lifecycle as `ChatPanel`'s socket -- opened on mount, closed on unmount,
 * no reconnect loop -- since a missed toast while briefly disconnected just
 * means the invite still shows up in `PendingInvites` on next load.
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

      // `kind`/`invite_id` (`rbac`/`meetings`' `notify_invite`) are what let
      // clicking the toast actually accept the invite instead of just
      // showing a message -- see `Toast`'s `onAccept`.
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
