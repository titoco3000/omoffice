import { useInviteActions } from "../hooks/useInviteActions";
import { IconClose } from "../ui/icons";

/**
 * A fixed stack of dismissible toasts, above every page. An invite toast is
 * clickable: accepting it here is the same call `PendingInvites` makes.
 */
export default function Toast({ toasts, onDismiss }) {
  const { acceptOrg, acceptMeeting } = useInviteActions();

  if (toasts.length === 0) return null;

  function accept(toast) {
    onDismiss(toast.id);
    if (!toast.inviteId) return;
    if (toast.kind === "meeting") acceptMeeting(toast.inviteId);
    else if (toast.kind === "org") acceptOrg(toast.inviteId);
  }

  return (
    <div className="fixed top-4 right-4 z-50 grid w-72 gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className="flex items-center gap-3 rounded-card border border-line bg-surface px-3 py-2 text-sm text-ink shadow-lg"
        >
          {toast.inviteId ? (
            <button
              type="button"
              onClick={() => accept(toast)}
              className="min-w-0 flex-1 text-left text-brand hover:underline"
            >
              {toast.message}
            </button>
          ) : (
            <span className="min-w-0 flex-1">{toast.message}</span>
          )}
          <button
            type="button"
            onClick={() => onDismiss(toast.id)}
            aria-label="dismiss"
            className="shrink-0 text-ink-faint hover:text-ink"
          >
            <IconClose className="h-4 w-4" />
          </button>
        </div>
      ))}
    </div>
  );
}
