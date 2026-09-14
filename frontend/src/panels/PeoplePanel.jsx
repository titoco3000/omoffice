import { useState } from "react";
import { decodeJwt } from "../lib/jwt";

// Everyone online in this org, grouped by whichever meeting (if any) they're
// currently in -- crossing `core::rbac`'s roster (backend/lib/core/rbac/mod.rs)
// with `session.meetings`' own attendee lists
// (`features::meetings::MeetingWithAttendees`, backend/lib/features/meetings/mod.rs).
// Clicking a member reveals the one action that applies to them: join their
// meeting, or invite them to one (starting the viewer's own if they don't
// have one yet).
export default function PeoplePanel({ token, session, members }) {
  const myId = decodeJwt(token)?.sub;
  const { meetings, myMeetingId, join, inviteUser, forceUser } = session;
  const [selected, setSelected] = useState(null);

  const meetingIdByUser = {};
  for (const meeting of meetings) {
    for (const userId of meeting.attendees ?? []) {
      meetingIdByUser[userId] = meeting.id;
    }
  }

  const online = members.filter(
    (member) => member.online && member.user_id !== myId,
  );
  const groups = new Map(); // meeting_id -> [member, ...]
  const alone = [];
  for (const member of online) {
    const meetingId = meetingIdByUser[member.user_id];
    if (meetingId) {
      if (!groups.has(meetingId)) groups.set(meetingId, []);
      groups.get(meetingId).push(member);
    } else {
      alone.push(member);
    }
  }

  const toggle = (userId) =>
    setSelected((current) => (current === userId ? null : userId));

  const renderMember = (member, meetingId) => (
    <li key={member.user_id} className="rounded-card border px-3 py-2">
      <button
        onClick={() => toggle(member.user_id)}
        className="w-full text-left text-ink"
      >
        {member.display_name}
      </button>
      {selected === member.user_id && (
        <div className="mt-2 flex flex-wrap gap-2">
          {meetingId ? (
            <button
              onClick={() => join(meetingId)}
              disabled={!!myMeetingId}
              className="rounded-card bg-brand px-3 py-1 text-sm text-on-brand hover:bg-brand-strong disabled:opacity-50"
            >
              Join their meeting
            </button>
          ) : (
            <button
              onClick={() => inviteUser(member.user_id)}
              className="rounded-card bg-brand px-3 py-1 text-sm text-on-brand hover:bg-brand-strong"
            >
              Invite to a meeting
            </button>
          )}
          {/* No accept step behind this -- see `features::meetings::force_join`
              (backend/lib/features/meetings/mod.rs). Only works if this tab
              ends up the meeting's creator (a fresh one, or one it already
              created), which the backend itself enforces regardless. */}
          <button
            onClick={() => forceUser(member.user_id)}
            className="rounded-card border border-brand px-3 py-1 text-sm text-brand hover:bg-brand/10"
          >
            Force into meeting
          </button>
        </div>
      )}
    </li>
  );

  return (
    <section className="rounded-card border bg-surface p-5">
      <h2 className="text-lg font-semibold text-ink">People</h2>

      {online.length === 0 && (
        <p className="mt-3 text-sm text-ink-muted">No one else is online.</p>
      )}

      {[...groups.entries()].map(([meetingId, group]) => (
        <div key={meetingId} className="mt-4">
          <h3 className="text-xs font-medium uppercase text-ink-muted">
            In a meeting
          </h3>
          <ul className="mt-1.5 space-y-1.5 text-sm">
            {group.map((member) => renderMember(member, meetingId))}
          </ul>
        </div>
      ))}

      {alone.length > 0 && (
        <div className="mt-4">
          {groups.size > 0 && (
            <h3 className="text-xs font-medium uppercase text-ink-muted">
              Online
            </h3>
          )}
          <ul className="mt-1.5 space-y-1.5 text-sm">
            {alone.map((member) => renderMember(member, null))}
          </ul>
        </div>
      )}
    </section>
  );
}
