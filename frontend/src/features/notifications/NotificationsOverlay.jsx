//! Global "wants a yes/no, or just wants to be seen" feed -- pending org
//! invites, pending meeting invites, and meeting access requests -- as one
//! floating column of bubbles over the content area. Mounted once by
//! `NavContainer`, so it's above every authenticated page, and above the
//! meeting dock (`z-[60]` beats `MeetingDock`'s `z-50`) rather than
//! something a maximized meeting could cover.
//!
//! The column itself is `pointer-events-none` and transparent; only each
//! bubble turns `pointer-events-auto` back on, so an empty stretch of it
//! never blocks whatever's underneath -- the sidebar, the meeting controls,
//! the mobile bar. Its edges dodge whichever of those bars is currently
//! playing "navbar" (see `edge`/`bottom` below) so a bubble can't render on
//! top of one either.
//!
//! A bubble's X only ever hides it from this tab -- `dismissedIds` is local
//! state, not a request to the backend. Declining an org/meeting invite is
//! its own explicit button, since that's a real, not-undoable action;
//! dismissing an access request (which is already frontend-only, see
//! `useMeetingSession.dismissAccessRequest`) is the one case where the X
//! and "no thanks" are the same thing.

import { useState } from "react";
import { FiX } from "react-icons/fi";
import { useAuth } from "../../core/auth/AuthContext";
import { useOrgSession, preferredName, useOrgInvites } from "../org";
import { useMeetingSessionContext } from "../meetings";
import { useIsMobile, useIsPortrait } from "../../core/hooks/useViewport";
import AsyncButton from "../../core/ui/AsyncButton";

function Bubble({ onClose, children }) {
  return (
    <li className="pointer-events-auto relative rounded-card border border-brand/40 bg-surface p-4 shadow-lg">
      {onClose && (
        <button
          onClick={onClose}
          title="Dismiss"
          className="absolute right-2 top-2 text-ink-faint hover:text-ink"
        >
          <FiX />
        </button>
      )}
      <div className="pr-5">{children}</div>
    </li>
  );
}

function OrgInviteBubble({ invite, busy, onAccept, onDecline, onClose }) {
  const resolved = invite.status && invite.status !== "pending";
  return (
    <Bubble onClose={onClose}>
      <p className="text-sm text-ink">
        You&apos;re invited to{" "}
        <span className="font-semibold">{invite.org_name}</span>
        {invite.roles?.length > 0 && <> as {invite.roles.join(", ")}</>}.
      </p>
      {resolved ? (
        <p className="mt-1 text-sm text-ink-muted">
          This invite was already {invite.status}.
        </p>
      ) : (
        <div className="mt-3 flex gap-2">
          <AsyncButton
            onClick={onAccept}
            disabled={busy}
            className="rounded-card bg-brand px-3 py-1 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
          >
            Accept
          </AsyncButton>
          <AsyncButton
            onClick={onDecline}
            className="rounded-card border px-3 py-1 text-sm text-ink-muted hover:text-ink"
          >
            Decline
          </AsyncButton>
        </div>
      )}
    </Bubble>
  );
}

function MeetingInviteBubble({
  invite,
  members,
  busy,
  onAccept,
  onDecline,
  onClose,
}) {
  return (
    <Bubble onClose={onClose}>
      <p className="text-sm text-ink">
        <span className="font-medium">
          {preferredName(invite.invited_by, members)}
        </span>{" "}
        invited you to a meeting.
      </p>
      <div className="mt-3 flex gap-2">
        <AsyncButton
          onClick={onAccept}
          disabled={busy}
          className="rounded-card bg-brand px-3 py-1 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
        >
          Accept
        </AsyncButton>
        <AsyncButton
          onClick={onDecline}
          className="rounded-card border px-3 py-1 text-sm text-ink-muted hover:text-ink"
        >
          Decline
        </AsyncButton>
      </div>
    </Bubble>
  );
}

function AccessRequestBubble({ userId, members, onAdmit, onDismiss }) {
  return (
    <Bubble onClose={onDismiss}>
      <p className="text-sm text-ink">
        <span className="font-medium">{preferredName(userId, members)}</span>{" "}
        wants to join your meeting.
      </p>
      <AsyncButton
        onClick={onAdmit}
        className="mt-3 rounded-card bg-brand px-3 py-1 text-sm font-medium text-on-brand hover:bg-brand-strong"
      >
        Let them in
      </AsyncButton>
    </Bubble>
  );
}

export default function NotificationsOverlay() {
  const { token } = useAuth();
  const { reloadOrgs, selectOrg, roster } = useOrgSession();
  const orgInvites = useOrgInvites(token);
  const {
    invites: meetingInvites,
    myMeetingId,
    acceptInvite: acceptMeeting,
    declineInvite: declineMeeting,
    accessRequests,
    admitAccessRequest,
    dismissAccessRequest,
  } = useMeetingSessionContext();
  const isMobile = useIsMobile();
  const isPortrait = useIsPortrait();
  const [dismissedIds, setDismissedIds] = useState(new Set());
  const dismiss = (id) => setDismissedIds((prev) => new Set(prev).add(id));

  const onOrgAccepted = (orgId) => {
    reloadOrgs();
    selectOrg(orgId);
  };

  const showUrlInvite =
    orgInvites.urlInvite && !dismissedIds.has(orgInvites.urlInviteId);
  const orgInviteList = orgInvites.invites.filter(
    (invite) => !dismissedIds.has(invite.id),
  );
  const meetingInviteList = meetingInvites.filter(
    (invite) => !dismissedIds.has(invite.id),
  );

  const empty =
    !showUrlInvite &&
    orgInviteList.length === 0 &&
    meetingInviteList.length === 0 &&
    accessRequests.length === 0;
  if (empty) return null;

  // Which edge/bottom the bar currently claiming "navbar" sits on -- see
  // `NavContainer`: a vertical bar on the right in mobile landscape, a
  // horizontal one at the bottom in mobile portrait, none at all on
  // desktop (the sidebar is on the left, out of this column's way).
  const edge = isMobile && !isPortrait ? "right-14" : "right-0";
  const bottom = isMobile && isPortrait ? "bottom-14" : "bottom-0";

  return (
    <ul
      className={`pointer-events-none fixed top-0 ${bottom} ${edge} z-60 flex w-chat max-w-full flex-col gap-2 overflow-y-auto p-3`}
    >
      {showUrlInvite && (
        <OrgInviteBubble
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
          onClose={() => dismiss(orgInvites.urlInviteId)}
        />
      )}
      {orgInviteList.map((invite) => (
        <OrgInviteBubble
          key={invite.id}
          invite={invite}
          busy={orgInvites.busyId === invite.id}
          onAccept={() =>
            orgInvites.accept(invite.id, invite.org_id, onOrgAccepted)
          }
          onDecline={() => orgInvites.decline(invite.id, invite.org_id)}
          onClose={() => dismiss(invite.id)}
        />
      ))}
      {meetingInviteList.map((invite) => (
        <MeetingInviteBubble
          key={invite.id}
          invite={invite}
          members={roster.members}
          busy={!!myMeetingId && myMeetingId !== invite.meeting_id}
          onAccept={() => acceptMeeting(invite.id)}
          onDecline={() => declineMeeting(invite.id)}
          onClose={() => dismiss(invite.id)}
        />
      ))}
      {accessRequests.map((request) => (
        <AccessRequestBubble
          key={request.userId}
          userId={request.userId}
          members={roster.members}
          onAdmit={() => admitAccessRequest(request.userId)}
          onDismiss={() => dismissAccessRequest(request.userId)}
        />
      ))}
      {orgInvites.error && (
        <li className="pointer-events-none text-sm text-danger">
          {orgInvites.error}
        </li>
      )}
    </ul>
  );
}
