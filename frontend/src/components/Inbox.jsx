//! Everything addressed to this account that wants a yes/no: pending org
//! invites (including the one riding along on a magic link's `?invite=`
//! param) and pending meeting invites for whichever org is current. Renders
//! nothing when both are empty.

import { useEffect, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { useOrgSession } from "../org/OrgSessionContext";
import { preferredName } from "../lib/names";
import {
  getInvite,
  acceptInvite,
  declineInvite,
  listMyInvites,
} from "../lib/rbac";

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

function InboxCard({ children }) {
  return (
    <li className="rounded-card border border-brand/40 bg-surface p-4">
      {children}
    </li>
  );
}

function OrgInviteCard({ invite, busy, onAccept, onDecline }) {
  return (
    <InboxCard>
      <p className="text-ink">
        You&apos;re invited to{" "}
        <span className="font-semibold">{invite.org_name}</span>
        {invite.roles?.length > 0 && <> as {invite.roles.join(", ")}</>}.
      </p>
      {invite.status && invite.status !== "pending" ? (
        <p className="mt-1 text-sm text-ink-muted">
          This invite was already {invite.status}.
        </p>
      ) : (
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
    </InboxCard>
  );
}

function MeetingInviteCard({ invite, members, busy, onAccept, onDecline }) {
  return (
    <InboxCard>
      <div className="flex items-center justify-between gap-3">
        <span className="text-ink">
          <span className="font-medium">
            {preferredName(invite.invited_by, members)}
          </span>{" "}
          invited you to a meeting
        </span>
        <div className="flex shrink-0 gap-2">
          <button
            onClick={onAccept}
            disabled={busy}
            className="rounded-card bg-brand px-3 py-1 text-sm text-on-brand hover:bg-brand-strong disabled:opacity-50"
          >
            Accept
          </button>
          <button
            onClick={onDecline}
            className="rounded-card border px-3 py-1 text-sm text-ink-muted hover:text-ink"
          >
            Decline
          </button>
        </div>
      </div>
    </InboxCard>
  );
}

// Every pending org invite for this account, not just the one riding along
// on `?invite=org:<id>` -- issued-before-signup and other-tab invites have
// no such param, only `listMyInvites` covers them.
function useOrgInvites(token) {
  const [urlInviteId] = useState(inviteIdFromUrl);
  const [urlInvite, setUrlInvite] = useState(null);
  const [urlDismissed, setUrlDismissed] = useState(false);
  const [invites, setInvites] = useState([]);
  const [busyId, setBusyId] = useState(null);
  const [error, setError] = useState(null);

  const reload = () =>
    listMyInvites(token)
      .then((data) => setInvites(data.invites ?? []))
      .catch((err) => setError(err.message));

  useEffect(() => {
    if (!urlInviteId) return;
    getInvite(token, urlInviteId)
      .then((data) => setUrlInvite(data.invite))
      .catch((err) => setError(err.message));
  }, [token, urlInviteId]);

  useEffect(() => {
    reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);

  const resolve = async (inviteId, orgId, action, onDone) => {
    setBusyId(inviteId);
    try {
      await action(token, inviteId);
      if (inviteId === urlInviteId) {
        clearInviteParam();
        setUrlDismissed(true);
      }
      setInvites((current) => current.filter((i) => i.id !== inviteId));
      onDone(orgId);
    } catch (err) {
      setError(err.message);
    } finally {
      setBusyId(null);
    }
  };

  const accept = (inviteId, orgId, onAccepted) =>
    resolve(inviteId, orgId, acceptInvite, onAccepted);
  const decline = (inviteId, orgId) =>
    resolve(inviteId, orgId, declineInvite, () => {});

  const listHasUrlInvite =
    urlInviteId && invites.some((i) => i.id === urlInviteId);
  const showUrlCard =
    urlInviteId && !urlDismissed && !listHasUrlInvite && urlInvite;

  return {
    invites,
    urlInvite: showUrlCard ? urlInvite : null,
    urlInviteId,
    busyId,
    error,
    accept,
    decline,
  };
}

export default function Inbox() {
  const { token } = useAuth();
  const { reloadOrgs, selectOrg, session, roster } = useOrgSession();
  const orgInvites = useOrgInvites(token);
  const {
    invites: meetingInvites,
    myMeetingId,
    acceptInvite: acceptMeeting,
    declineInvite: declineMeeting,
  } = session;

  const onOrgAccepted = (orgId) => {
    reloadOrgs();
    selectOrg(orgId);
  };

  const empty =
    !orgInvites.urlInvite &&
    orgInvites.invites.length === 0 &&
    meetingInvites.length === 0;
  if (empty) return null;

  return (
    <section>
      <h2 className="text-lg font-semibold text-ink">Inbox</h2>
      <ul className="mt-3 space-y-2">
        {orgInvites.urlInvite && (
          <OrgInviteCard
            invite={orgInvites.urlInvite}
            busy={orgInvites.busyId === orgInvites.urlInviteId}
            onAccept={() =>
              orgInvites.accept(
                orgInvites.urlInviteId,
                orgInvites.urlInvite.org_id,
                onOrgAccepted,
              )
            }
            onDecline={() =>
              orgInvites.decline(
                orgInvites.urlInviteId,
                orgInvites.urlInvite.org_id,
              )
            }
          />
        )}
        {orgInvites.invites.map((invite) => (
          <OrgInviteCard
            key={invite.id}
            invite={invite}
            busy={orgInvites.busyId === invite.id}
            onAccept={() =>
              orgInvites.accept(invite.id, invite.org_id, onOrgAccepted)
            }
            onDecline={() => orgInvites.decline(invite.id, invite.org_id)}
          />
        ))}
        {meetingInvites.map((invite) => (
          <MeetingInviteCard
            key={invite.id}
            invite={invite}
            members={roster.members}
            busy={!!myMeetingId && myMeetingId !== invite.meeting_id}
            onAccept={() => acceptMeeting(invite.id)}
            onDecline={() => declineMeeting(invite.id)}
          />
        ))}
      </ul>
      {orgInvites.error && (
        <p className="mt-2 text-sm text-danger">{orgInvites.error}</p>
      )}
    </section>
  );
}
