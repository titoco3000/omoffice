/**
 * A fixed-position stack of dismissible toasts. An invite toast is
 * clickable: `onAccept` wires into `useInviteActions` to accept it.
 */
export default function Toast({ toasts, onDismiss, onAccept }) {
  if (toasts.length === 0) return null;

  return (
    <div className="fixed right-4 top-4 z-50 grid gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className="flex items-center gap-3 rounded-lg border border-sky-800 bg-slate-900 px-4 py-2 text-sm text-slate-100 shadow-lg"
        >
          {toast.inviteId ? (
            <button
              type="button"
              onClick={() => onAccept?.(toast)}
              className="text-left text-sky-300 hover:text-sky-200"
            >
              {toast.message}
            </button>
          ) : (
            <span>{toast.message}</span>
          )}
          <button
            type="button"
            onClick={() => onDismiss(toast.id)}
            className="ml-auto text-slate-500 hover:text-slate-300"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
