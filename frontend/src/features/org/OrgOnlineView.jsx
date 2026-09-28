//! Everyone online in the current org, grouped by whichever meeting (if
//! any) they're in, plus who's offline. Clicking an online member reveals
//! whichever of join/invite/ask-to-join actually make sense for them --
//! see `actionsFor` below, which is the one place that decision is made.
//! Offline members are listed but inert -- there's nothing to do with them
//! yet. A member already sharing this tab's own meeting gets no actions
//! either -- nothing to join or invite them to that isn't already true.
//!
//! Takes the meeting session as a prop (`meetingSession`, the value of
//! `features/meetings`' `useMeetingSessionContext()`) instead of reading it
//! itself, so `features/org` never imports `features/meetings`.
//!
//! Collapses to first-letter avatars when the container gets too narrow to
//! fit a name.

import { useLayoutEffect, useRef, useState } from "react";
import { FiLock } from "react-icons/fi";
import { useOrgSession } from "./OrgSessionContext";
import { avatarUrl } from "../avatars";
import AsyncButton from "../../core/ui/AsyncButton";

const COMPACT_BELOW_PX = 176;

function Avatar({ name, picture, online }) {
  const url = avatarUrl(picture);
  if (url) {
    return (
      <img
        src={url}
        alt=""
        className={`size-7 shrink-0 rounded-full object-cover ${online ? "" : "opacity-50"}`}
      />
    );
  }
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

// Which actions to offer upon clicking on other org member
function actionsFor(myMeetingId, meetingId, locked) {
  if (meetingId && locked) return ["ask", "invite"];
  return myMeetingId ? ["join", "invite"] : ["join"];
}

const ACTION_LABEL = {
  join: "Join",
  invite: "Invite",
  ask: "Ask to join",
};

function MemberRow({
  member,
  meetingId,
  meetingLocked,
  myMeetingId,
  compact,
  onJoin,
  onRequestAccess,
  onInvite,
}) {
  const [open, setOpen] = useState(false);
  const sameMeeting = !!meetingId && meetingId === myMeetingId;
  const interactive = member.online && !sameMeeting;

  // Dropdown stays open (buttons showing their own spinner/disabled state
  // via `AsyncButton`) until the action actually finishes, rather than
  // closing on click and hiding that state.
  const runAndClose = async (action) => {
    await action();
    setOpen(false);
  };

  const HANDLER = {
    join: () => onJoin(member.id, meetingId),
    invite: () => onInvite(member.id),
    ask: () => onRequestAccess(meetingId),
  };

  return (
    <li className="relative">
      <button
        onClick={interactive ? () => setOpen((v) => !v) : undefined}
        title={member.name}
        className={`flex w-full items-center gap-2 rounded-card px-2 py-1.5 text-left ${
          interactive ? "hover:bg-muted" : "cursor-default opacity-60"
        }`}
      >
        <Avatar
          name={member.name}
          picture={member.picture}
          online={member.online}
        />
        {!compact && (
          <span className="truncate text-sm text-ink">{member.name}</span>
        )}
      </button>
      {open && (
        <div className="absolute left-0 z-10 mt-1 flex min-w-max flex-col gap-1 rounded-card border bg-surface p-1.5 shadow-lg">
          {actionsFor(myMeetingId, meetingId, meetingLocked).map((action) => (
            <AsyncButton
              key={action}
              onClick={() => runAndClose(HANDLER[action])}
              className="whitespace-nowrap rounded-card px-2 py-1 text-left text-sm text-ink hover:bg-muted"
            >
              {ACTION_LABEL[action]}
            </AsyncButton>
          ))}
        </div>
      )}
    </li>
  );
}

export default function OrgOnlineView({ meetingSession }) {
  const { myId, roster } = useOrgSession();
  const { meetings, myMeetingId, joinOrForce, requestAccess, inviteUser } =
    meetingSession;
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
  const meetingById = new Map();
  for (const meeting of meetings) {
    meetingById.set(meeting.id, meeting);
    for (const userId of meeting.attendees ?? [])
      meetingIdByUser[userId] = meeting.id;
  }

  const others = roster.members.filter((member) => member.id !== myId);
  const online = others.filter((member) => member.online);
  const offline = others.filter((member) => !member.online);

  const groups = new Map(); // meeting_id -> [member, ...]
  const alone = [];
  for (const member of online) {
    const meetingId = meetingIdByUser[member.id];
    if (meetingId) {
      if (!groups.has(meetingId)) groups.set(meetingId, []);
      groups.get(meetingId).push(member);
    } else {
      alone.push(member);
    }
  }

  const row = (member, meetingId = null) => (
    <MemberRow
      key={member.id}
      member={member}
      meetingId={meetingId}
      meetingLocked={!!meetingById.get(meetingId)?.invite_only}
      myMeetingId={myMeetingId}
      compact={compact}
      onJoin={joinOrForce}
      onRequestAccess={requestAccess}
      onInvite={inviteUser}
    />
  );

  return (
    <div ref={containerRef} className="flex flex-col gap-4">
      {[...groups.entries()].map(([meetingId, group]) => (
        <div key={meetingId}>
          {!compact && (
            <h3 className="flex items-center gap-1 px-2 text-xs font-medium uppercase text-ink-muted">
              In a meeting
              {meetingById.get(meetingId)?.invite_only && (
                <FiLock title="Locked" />
              )}
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
