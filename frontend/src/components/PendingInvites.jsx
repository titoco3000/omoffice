import { useEffect, useState } from "react";
import { callApi, describeFailure } from "../api";
import { useAuth } from "../auth/AuthContext";
import { useInviteActions } from "../hooks/useInviteActions";

/**
 * `GET /private/invites` -- every org and meeting invite still pending for
 * the logged-in email, for whoever logged in on their own instead of
 * following a magic link. Same "stamp" pattern as `useOrgs`/`MeetingHistory`.
 *
 * Accepting reaches the same public accept routes the magic link uses
 * (`/invites/org/{id}/accept`, `/invites/meetings/{id}/accept}`) and adopts
 * the token they hand back via `loginWithToken` -- for an org invite that's
 * the whole job; for a meeting invite `onJoinMeeting(code)` also hands the
 * code up to `Shell` so it can switch to the meetings tab with it prefilled.
 */
export default function PendingInvites({ onJoinMeeting }) {
  const { token } = useAuth();
  const { acceptOrg, acceptMeeting } = useInviteActions({ onJoinMeeting });
  const [reloads, setReloads] = useState(0);
  const [result, setResult] = useState({
    stamp: null,
    orgInvites: [],
    meetingInvites: [],
    error: "",
  });
  const [busyId, setBusyId] = useState(null);

  const stamp = `${token}:${reloads}`;

  useEffect(() => {
    let cancelled = false;
    callApi({ path: "/private/invites", token }).then((response) => {
      if (cancelled) return;
      setResult({
        stamp: `${token}:${reloads}`,
        orgInvites: response.ok ? (response.data.org_invites ?? []) : [],
        meetingInvites: response.ok ? (response.data.meeting_invites ?? []) : [],
        error: response.ok ? "" : describeFailure(response),
      });
    });
    return () => {
      cancelled = true;
    };
  }, [token, reloads]);

  const shown = result.stamp === stamp ? result : { orgInvites: [], meetingInvites: [], error: "" };
  const { orgInvites, meetingInvites, error } = shown;

  function refresh() {
    setReloads((count) => count + 1);
  }

  async function accept(invite) {
    setBusyId(invite.id);
    await acceptOrg(invite.id);
    setBusyId(null);
    refresh();
  }

  async function declineOrg(invite) {
    setBusyId(invite.id);
    await callApi({
      method: "POST",
      path: `/private/invites/org/${encodeURIComponent(invite.id)}/decline`,
      token,
    });
    setBusyId(null);
    refresh();
  }

  async function acceptOneMeeting(invite) {
    setBusyId(invite.id);
    await acceptMeeting(invite.id);
    setBusyId(null);
    refresh();
  }

  async function declineMeeting(invite) {
    setBusyId(invite.id);
    await callApi({
      method: "POST",
      path: `/private/invites/meetings/${encodeURIComponent(invite.id)}/decline`,
      token,
    });
    setBusyId(null);
    refresh();
  }

  // Silent when there's nothing to show -- a fetch error here shouldn't
  // block the rest of the app the way `Organizations`/`MeetingPanel`'s own
  // errors surface inline in their own sections.
  if (error || (orgInvites.length === 0 && meetingInvites.length === 0)) return null;

  return (
    <section className="mb-6 rounded-xl border border-amber-800 bg-amber-950/30 p-4">
      <h2 className="text-sm font-medium text-amber-300">pending invites</h2>
      <ul className="mt-2 grid gap-2">
        {orgInvites.map((invite) => (
          <li
            key={invite.id}
            className="flex flex-wrap items-center gap-2 rounded-lg bg-slate-950 px-3 py-2"
          >
            <span className="min-w-0 flex-1 truncate text-sm text-slate-200">
              {invite.invited_by} invited you to <strong>{invite.org_name}</strong>
            </span>
            <button
              type="button"
              disabled={busyId === invite.id}
              onClick={() => accept(invite)}
              className="rounded-lg bg-sky-600 px-3 py-1 text-xs font-medium text-white hover:bg-sky-500 disabled:opacity-50"
            >
              accept
            </button>
            <button
              type="button"
              disabled={busyId === invite.id}
              onClick={() => declineOrg(invite)}
              className="rounded-lg bg-slate-700 px-3 py-1 text-xs text-white hover:bg-slate-600 disabled:opacity-50"
            >
              decline
            </button>
          </li>
        ))}
        {meetingInvites.map((invite) => (
          <li
            key={invite.id}
            className="flex flex-wrap items-center gap-2 rounded-lg bg-slate-950 px-3 py-2"
          >
            <span className="min-w-0 flex-1 truncate text-sm text-slate-200">
              {invite.invited_by} invited you to a meeting
            </span>
            <button
              type="button"
              disabled={busyId === invite.id}
              onClick={() => acceptOneMeeting(invite)}
              className="rounded-lg bg-sky-600 px-3 py-1 text-xs font-medium text-white hover:bg-sky-500 disabled:opacity-50"
            >
              join
            </button>
            <button
              type="button"
              disabled={busyId === invite.id}
              onClick={() => declineMeeting(invite)}
              className="rounded-lg bg-slate-700 px-3 py-1 text-xs text-white hover:bg-slate-600 disabled:opacity-50"
            >
              decline
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
