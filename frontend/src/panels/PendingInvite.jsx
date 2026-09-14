import { useEffect, useState } from "react";
import {
  getInvite,
  acceptInvite,
  declineInvite,
  listMyInvites,
} from "../lib/rbac";

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

function InviteCard({ invite, busy, onAccept, onDecline, error }) {
  return (
    <section className="rounded-card border border-brand/40 bg-surface p-4">
      <p className="text-ink">
        You&apos;re invited to{" "}
        <span className="font-semibold">{invite.org_name}</span>
        {invite.roles.length > 0 && <> as {invite.roles.join(", ")}</>}.
      </p>

      {invite.status !== "pending" && (
        <p className="mt-1 text-sm text-ink-muted">
          This invite was already {invite.status}.
        </p>
      )}

      {invite.status === "pending" && (
        <div className="mt-3 flex gap-2">
          <button
            onClick={onAccept}
            disabled={busy}
            className="rounded-card bg-brand px-4 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
          >
            Accept
          </button>
          <button
            onClick={onDecline}
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

// Every pending org invite for this account, not just the one riding along
// on `?invite=org:<id>` -- that param only ever names the invite behind
// whichever magic link opened this tab, so it misses every other invite:
// issued before this account existed, or while this browser was never
// online to receive `[core::rbac::org_invites]`'s live push.
export default function PendingInvite({ token, onAccepted, onDeclined }) {
  const [urlInviteId] = useState(inviteIdFromUrl);
  const [urlInvite, setUrlInvite] = useState(null);
  const [urlError, setUrlError] = useState(null);
  const [urlDismissed, setUrlDismissed] = useState(false);

  const [invites, setInvites] = useState([]);
  const [busyId, setBusyId] = useState(null);
  const [listError, setListError] = useState(null);

  const reloadInvites = () =>
    listMyInvites(token)
      .then((data) => setInvites(data.invites ?? []))
      .catch((err) => setListError(err.message));

  useEffect(() => {
    if (!urlInviteId) return;
    getInvite(token, urlInviteId)
      .then((data) => setUrlInvite(data.invite))
      .catch((err) => setUrlError(err.message));
  }, [token, urlInviteId]);

  useEffect(() => {
    reloadInvites();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);

  const accept = async (inviteId, orgId) => {
    setBusyId(inviteId);
    try {
      await acceptInvite(token, inviteId);
      if (inviteId === urlInviteId) {
        clearInviteParam();
        setUrlDismissed(true);
      }
      setInvites((current) => current.filter((i) => i.id !== inviteId));
      onAccepted(orgId);
    } catch (err) {
      setListError(err.message);
    } finally {
      setBusyId(null);
    }
  };

  const decline = async (inviteId) => {
    setBusyId(inviteId);
    try {
      await declineInvite(token, inviteId);
      if (inviteId === urlInviteId) {
        clearInviteParam();
        setUrlDismissed(true);
      }
      setInvites((current) => current.filter((i) => i.id !== inviteId));
      onDeclined();
    } catch (err) {
      setListError(err.message);
    } finally {
      setBusyId(null);
    }
  };

  // The list already covers every pending invite, url one included -- so
  // once it's loaded, show cards from it alone and only fall back to the
  // url-only fetch for an invite the list can't (one already resolved
  // before this tab reloaded it, which `getInvite` still reports status for).
  const listHasUrlInvite =
    urlInviteId && invites.some((i) => i.id === urlInviteId);
  const showUrlCard =
    urlInviteId && !urlDismissed && !listHasUrlInvite && (urlInvite || urlError);

  return (
    <>
      {showUrlCard && urlInvite && (
        <InviteCard
          invite={urlInvite}
          busy={busyId === urlInviteId}
          onAccept={() => accept(urlInviteId, urlInvite.org_id)}
          onDecline={() => decline(urlInviteId)}
          error={urlError}
        />
      )}
      {showUrlCard && !urlInvite && urlError && (
        <p className="text-sm text-danger">{urlError}</p>
      )}
      {invites.map((invite) => (
        <InviteCard
          key={invite.id}
          invite={invite}
          busy={busyId === invite.id}
          onAccept={() => accept(invite.id, invite.org_id)}
          onDecline={() => decline(invite.id)}
        />
      ))}
      {listError && <p className="text-sm text-danger">{listError}</p>}
    </>
  );
}
