import { useRef, useState } from "react";
import { CHANNELS, useChannel } from "../messenger/MessengerContext";

/// How long a toast stays before it dismisses itself.
const TOAST_LIFETIME_MS = 8000;

/**
 * The live half of an invite's delivery: every message on the `invites`
 * channel becomes a toast. Which socket carried it, and whether there was
 * one at all, is the messenger's business.
 */
export function useNotifications() {
  const [toasts, setToasts] = useState([]);
  const nextIdRef = useRef(0);

  useChannel(CHANNELS.invites, (message) => {
    if (typeof message?.message !== "string") return;

    // `kind`/`invite_id` are what let clicking the toast accept the
    // invite instead of just showing a message — see `Toast`'s `onAccept`.
    nextIdRef.current += 1;
    const id = nextIdRef.current;
    setToasts((current) => [
      ...current,
      { id, message: message.message, kind: message.kind, inviteId: message.invite_id },
    ]);
    setTimeout(() => {
      setToasts((current) => current.filter((toast) => toast.id !== id));
    }, TOAST_LIFETIME_MS);
  });

  function dismiss(id) {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }

  return { toasts, dismiss };
}
