import { useState } from "react";
import { callApi, describeFailure } from "../api";
import { useAuth } from "../auth/AuthContext";
import { useActiveMeeting } from "../meeting/MeetingContext";
import { useOrgRoster } from "../hooks/useOrgRoster";
import { useOrgContext } from "../org/OrgContext";
import { useNavigate } from "../router";
import Avatar from "../ui/Avatar";
import Badge from "../ui/Badge";
import Button from "../ui/Button";
import Dropdown, { MenuItem } from "../ui/Dropdown";
import Notice from "../ui/Notice";
import { IconMeeting, IconPlus } from "../ui/icons";

/**
 * Who is in the current org and which meetings are live — the sidebar's
 * body, and the whole of `/users` on a phone.
 *
 * Clicking a person is a verb, not navigation: with no meeting of your own
 * it creates one and invites them; with one already running the click opens
 * the two things you can do from inside it. `compact` is the collapsed
 * sidebar: avatars only, same actions.
 */
export default function OrgUserList({ compact = false }) {
  const { token, email: myEmail } = useAuth();
  const { selectedOrgId, selectedOrg } = useOrgContext();
  const { people, meetings, error, refresh } = useOrgRoster(token, selectedOrgId);
  const { code: activeCode, join } = useActiveMeeting();
  const navigate = useNavigate();

  const [busy, setBusy] = useState("");
  const [notice, setNotice] = useState("");

  function enter(code) {
    join(code);
    navigate(`/meeting/${encodeURIComponent(code)}`);
  }

  /// Create a meeting under the current org, optionally inviting one person
  /// into it, and go there. The invite is a separate call because the
  /// backend gates it on being the meeting's creator, which is only true
  /// once the meeting exists.
  async function startMeeting(withEmail) {
    if (!selectedOrgId) return;
    setBusy(withEmail ?? "new");
    setNotice("");

    const created = await callApi({
      method: "POST",
      path: "/private/meetings",
      token,
      body: { org_id: selectedOrgId },
    });
    if (!created.ok) {
      setBusy("");
      setNotice(describeFailure(created));
      return;
    }

    const code = created.data.code;
    if (withEmail) await invite(code, withEmail);
    setBusy("");
    enter(code);
  }

  async function invite(code, email) {
    const response = await callApi({
      method: "POST",
      path: `/private/meetings/${encodeURIComponent(code)}/invites`,
      token,
      body: { email },
    });
    if (!response.ok) setNotice(describeFailure(response));
    else setNotice(`invited ${email}`);
  }

  async function inviteToActive(email) {
    setBusy(email);
    setNotice("");
    await invite(activeCode, email);
    setBusy("");
  }

  if (!selectedOrg) {
    return compact ? null : (
      <p className="px-2 py-3 text-xs text-ink-faint">
        no org yet — create one from your profile
      </p>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      <section>
        <SectionTitle
          compact={compact}
          label="meetings"
          action={
            <Button
              size="icon-sm"
              variant="ghost"
              title="start a meeting"
              aria-label="start a meeting"
              disabled={busy === "new"}
              onClick={() => startMeeting(null)}
            >
              <IconPlus className="h-4 w-4" />
            </Button>
          }
        />

        {meetings.length === 0 && !compact && (
          <p className="px-2 text-xs text-ink-faint">none live</p>
        )}

        <ul className="grid min-w-0 gap-1">
          {meetings.map((meeting) => (
            <li key={meeting.code} className="min-w-0">
              <button
                type="button"
                onClick={() => enter(meeting.code)}
                title={`${meeting.code} — ${meeting.participants.join(", ")}`}
                className={`flex w-full min-w-0 items-center gap-2 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-muted ${
                  meeting.code === activeCode ? "bg-muted" : ""
                } ${compact ? "justify-center" : ""}`}
              >
                <IconMeeting className="h-4 w-4 shrink-0 text-success" />
                {!compact && (
                  <>
                    <span className="min-w-0 flex-1 truncate font-mono text-xs text-ink-muted">
                      {meeting.code}
                    </span>
                    <Badge tone="success">{meeting.participants.length}</Badge>
                  </>
                )}
              </button>
            </li>
          ))}
        </ul>
      </section>

      <section className="flex min-h-0 flex-1 flex-col">
        <SectionTitle compact={compact} label="people" />

        <ul className="grid min-h-0 min-w-0 flex-1 gap-1 overflow-x-hidden overflow-y-auto">
          {people.map((person) => (
            <li key={person.email} className="min-w-0">
              <PersonRow
                person={person}
                compact={compact}
                you={person.email === myEmail}
                busy={busy === person.email}
                inMeeting={Boolean(activeCode)}
                onStart={() => startMeeting(person.email)}
                onInvite={() => inviteToActive(person.email)}
                onSwap={() => startMeeting(person.email)}
              />
            </li>
          ))}
        </ul>
      </section>

      {!compact && (error || notice) && (
        <Notice tone={error ? "danger" : "info"}>
          {error || notice}{" "}
          {error && (
            <button type="button" onClick={refresh} className="underline">
              retry
            </button>
          )}
        </Notice>
      )}
    </div>
  );
}

/// The label is what collapses, never the action beside it: a rail with no
/// way to start a meeting would be a rail you have to expand to use.
function SectionTitle({ compact, label, action }) {
  return (
    <div
      className={`mb-1 flex items-center gap-1 px-2 text-[11px] font-semibold tracking-wide text-ink-faint uppercase ${
        compact ? "justify-center" : ""
      }`}
    >
      <span className={compact ? "sr-only" : ""}>{label}</span>
      {action}
    </div>
  );
}

/**
 * One person. Without an active meeting the whole row is the "start a
 * meeting with them" button; with one it is a dropdown, because there are
 * then two different things the click could mean.
 */
function PersonRow({ person, compact, you, busy, inMeeting, onStart, onInvite, onSwap }) {
  const body = (
    <>
      <Avatar email={person.email} size="sm" online={person.online} />
      {!compact && (
        <>
          <span className="min-w-0 flex-1 truncate text-sm text-ink">
            {person.email}
            {you && <span className="ml-1 text-ink-faint">(you)</span>}
          </span>
          {person.meeting && <IconMeeting className="h-4 w-4 shrink-0 text-success" />}
        </>
      )}
    </>
  );

  const rowClass = `flex w-full min-w-0 items-center gap-2 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-muted disabled:opacity-50 ${
    compact ? "justify-center" : ""
  }`;

  if (!inMeeting || you) {
    return (
      <button
        type="button"
        onClick={you ? undefined : onStart}
        disabled={busy || you}
        title={you ? person.email : `start a meeting with ${person.email}`}
        className={rowClass}
      >
        {body}
      </button>
    );
  }

  return (
    <Dropdown
      align={compact ? "left" : "right"}
      trigger={({ toggle }) => (
        <button
          type="button"
          onClick={toggle}
          disabled={busy}
          title={person.email}
          className={rowClass}
        >
          {body}
        </button>
      )}
    >
      {(close) => (
        <>
          <MenuItem
            onClick={() => {
              close();
              onInvite();
            }}
          >
            invite to this meeting
          </MenuItem>
          <MenuItem
            tone="danger"
            onClick={() => {
              close();
              onSwap();
            }}
          >
            leave, start a new one
          </MenuItem>
        </>
      )}
    </Dropdown>
  );
}
