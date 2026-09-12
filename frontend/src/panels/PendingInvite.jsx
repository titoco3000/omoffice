import { useEffect, useState } from "react";
import { getInvite, acceptInvite, declineInvite } from "../lib/rbac";

// The magic link's `?invite=org:<id>` -- see `core::rbac::org_invites::create_invite`.
function inviteIdFromUrl() {
  const raw = new URLSearchParams(window.location.search).get("invite");
  if (!raw) return null;
  const [kind, id] = raw.split(":");
  return kind === "org" ? id : null;
}

function clearInviteParam() {
  const url = new URL(window.location.href);
  url.searchParams.delete("invite");
  window.history.replaceState({}, "", `${url.pathname}${url.search}`);
}

export default function PendingInvite({ token, onAccepted, onDeclined }) {
  const [inviteId] = useState(inviteIdFromUrl);
  const [invite, setInvite] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!inviteId) return;
    getInvite(token, inviteId)
      .then((data) => setInvite(data.invite))
      .catch((err) => setError(err.message));
  }, [token, inviteId]);

  if (!inviteId) return null;

  const accept = async () => {
    setBusy(true);
    try {
      await acceptInvite(token, inviteId);
      clearInviteParam();
      onAccepted(invite.org_id);
    } catch (err) {
      setError(err.message);
      setBusy(false);
    }
  };

  const decline = async () => {
    setBusy(true);
    try {
      await declineInvite(token, inviteId);
      clearInviteParam();
      onDeclined();
    } catch (err) {
      setError(err.message);
      setBusy(false);
    }
  };

  return (
    <section className="rounded-card border border-brand/40 bg-surface p-4">
      {!invite && !error && <p className="text-ink-muted">Loading invite…</p>}

      {invite && (
        <p className="text-ink">
          You&apos;re invited to{" "}
          <span className="font-semibold">{invite.org_name}</span>
          {invite.roles.length > 0 && <> as {invite.roles.join(", ")}</>}.
        </p>
      )}

      {invite && invite.status !== "pending" && (
        <p className="mt-1 text-sm text-ink-muted">
          This invite was already {invite.status}.
        </p>
      )}

      {invite?.status === "pending" && (
        <div className="mt-3 flex gap-2">
          <button
            onClick={accept}
            disabled={busy}
            className="rounded-card bg-brand px-4 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
          >
            Accept
          </button>
          <button
            onClick={decline}
            disabled={busy}
            className="rounded-card border px-4 py-1.5 text-sm text-ink-muted hover:text-ink disabled:opacity-50"
          >
            Decline
          </button>
        </div>
      )}

      {error && <p className="mt-2 text-sm text-danger">{error}</p>}
    </section>
  );
}
