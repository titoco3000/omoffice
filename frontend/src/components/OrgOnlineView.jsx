//! Everyone online in the current org, grouped by whichever meeting (if
//! any) they're in, plus who's offline. Clicking an online member reveals
//! join/invite/force-join actions for them; offline members are listed but
//! inert -- there's nothing to join, invite, or force yet.
//!
//! Collapses to first-letter avatars when the container gets too narrow to
//! fit a name.

import { useLayoutEffect, useRef, useState } from "react";
import { useOrgSession } from "../org/OrgSessionContext";
import AsyncButton from "./AsyncButton";

const COMPACT_BELOW_PX = 176;

function Avatar({ name, online }) {
  return (
    <span
      className={`flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${
        online ? "bg-brand text-on-brand" : "bg-muted text-ink-faint"
      }`}
    >
      {name.charAt(0).toUpperCase()}
    </span>
  );
}

function MemberRow({
  member,
  meetingId,
  myMeetingId,
  compact,
  onJoin,
  onInvite,
  onForce,
}) {
  const [open, setOpen] = useState(false);
  const interactive = member.online;

  // Dropdown stays open (buttons showing their own spinner/disabled state
  // via `AsyncButton`) until the action actually finishes, rather than
  // closing on click and hiding that state.
  const runAndClose = async (action) => {
    await action();
    setOpen(false);
  };

  return (
    <li className="relative">
      <button
        onClick={interactive ? () => setOpen((v) => !v) : undefined}
        title={member.display_name}
        className={`flex w-full items-center gap-2 rounded-card px-2 py-1.5 text-left ${
          interactive ? "hover:bg-muted" : "cursor-default opacity-60"
        }`}
      >
        <Avatar name={member.display_name} online={member.online} />
        {!compact && (
          <span className="truncate text-sm text-ink">
            {member.display_name}
          </span>
        )}
      </button>
      {open && (
        <div className="absolute left-0 z-10 mt-1 flex min-w-max flex-col gap-1 rounded-card border bg-surface p-1.5 shadow-lg">
          {meetingId ? (
            <AsyncButton
              onClick={() => runAndClose(() => onJoin(meetingId))}
              disabled={!!myMeetingId}
              className="whitespace-nowrap rounded-card px-2 py-1 text-left text-sm text-ink hover:bg-muted disabled:opacity-50"
            >
              Join their meeting
            </AsyncButton>
          ) : (
            <AsyncButton
              onClick={() => runAndClose(onInvite)}
              className="whitespace-nowrap rounded-card px-2 py-1 text-left text-sm text-ink hover:bg-muted"
            >
              Invite to a meeting
            </AsyncButton>
          )}
          <AsyncButton
            onClick={() => runAndClose(onForce)}
            className="whitespace-nowrap rounded-card px-2 py-1 text-left text-sm text-ink hover:bg-muted"
          >
            Force into meeting
          </AsyncButton>
        </div>
      )}
    </li>
  );
}

export default function OrgOnlineView() {
  const { myId, roster, session } = useOrgSession();
  const { meetings, myMeetingId, join, inviteUser, forceUser } = session;
  const containerRef = useRef(null);
  const [compact, setCompact] = useState(false);

  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) =>
      setCompact(entry.contentRect.width < COMPACT_BELOW_PX),
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const meetingIdByUser = {};
  for (const meeting of meetings) {
    for (const userId of meeting.attendees ?? [])
      meetingIdByUser[userId] = meeting.id;
  }

  const others = roster.members.filter((member) => member.user_id !== myId);
  const online = others.filter((member) => member.online);
  const offline = others.filter((member) => !member.online);

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

  const row = (member, meetingId = null) => (
    <MemberRow
      key={member.user_id}
      member={member}
      meetingId={meetingId}
      myMeetingId={myMeetingId}
      compact={compact}
      onJoin={join}
      onInvite={() => inviteUser(member.user_id)}
      onForce={() => forceUser(member.user_id)}
    />
  );

  return (
    <div ref={containerRef} className="flex flex-col gap-4">
      {[...groups.entries()].map(([meetingId, group]) => (
        <div key={meetingId}>
          {!compact && (
            <h3 className="px-2 text-xs font-medium uppercase text-ink-muted">
              In a meeting
            </h3>
          )}
          <ul className="mt-1 space-y-0.5">
            {group.map((member) => row(member, meetingId))}
          </ul>
        </div>
      ))}

      {alone.length > 0 && (
        <div>
          {!compact && groups.size > 0 && (
            <h3 className="px-2 text-xs font-medium uppercase text-ink-muted">
              Online
            </h3>
          )}
          <ul className="mt-1 space-y-0.5">
            {alone.map((member) => row(member))}
          </ul>
        </div>
      )}

      {offline.length > 0 && (
        <div>
          {!compact && (
            <h3 className="px-2 text-xs font-medium uppercase text-ink-muted">
              Offline
            </h3>
          )}
          <ul className="mt-1 space-y-0.5">
            {offline.map((member) => row(member))}
          </ul>
        </div>
      )}

      {others.length === 0 && !compact && (
        <p className="px-2 text-sm text-ink-muted">No other members yet.</p>
      )}
    </div>
  );
}
