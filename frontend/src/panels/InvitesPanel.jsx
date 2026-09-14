import { preferredName } from "../lib/names";

// Meeting invites addressed to this user -- see `features::meetings::meeting_invites`
// (backend/lib/features/meetings/mod.rs). Renders nothing at all when
// there are none, rather than an empty section.
//
// Named via `preferredName` against `members` rather than the backend's own
// `invite.invited_by_name` -- that one only ever falls back to the inviter's
// global name/email (see `meeting_invites::InviteRow`,
// backend/lib/features/meetings/meeting_invites.rs), since a meeting invite
// has no org-scoped query to join against. `members` does.
export default function InvitesPanel({ session, members }) {
  const { invites, myMeetingId, acceptInvite, declineInvite } = session;

  if (invites.length === 0) return null;

  return (
    <section className="rounded-card border border-brand/40 bg-surface p-4">
      <h2 className="text-lg font-semibold text-ink">Meeting invites</h2>
      <ul className="mt-2 space-y-2 text-sm">
        {invites.map((invite) => {
          const busy = !!myMeetingId && myMeetingId !== invite.meeting_id;
          return (
            <li
              key={invite.id}
              className="flex items-center justify-between gap-3"
            >
              <span className="text-ink">
                <span className="font-medium">
                  {preferredName(invite.invited_by, members)}
                </span>{" "}
                invited you to a meeting
              </span>
              <div className="flex shrink-0 gap-2">
                <button
                  onClick={() => acceptInvite(invite.id)}
                  disabled={busy}
                  className="rounded-card bg-brand px-3 py-1 text-on-brand hover:bg-brand-strong disabled:opacity-50"
                >
                  Accept
                </button>
                <button
                  onClick={() => declineInvite(invite.id)}
                  className="rounded-card border px-3 py-1 text-ink-muted hover:text-ink"
                >
                  Decline
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
