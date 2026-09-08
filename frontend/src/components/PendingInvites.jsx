import { useEffect, useState } from "react";
import { callApi } from "../api";
import { useAuth } from "../auth/AuthContext";
import { useInviteActions } from "../hooks/useInviteActions";
import Button from "../ui/Button";
import Card, { CardHeader } from "../ui/Card";

/**
 * `GET /private/invites`, for whoever logged in directly instead of through
 * a magic link. Accepting reuses `useInviteActions`'s token adoption, so an
 * accepted meeting invite lands in the meeting exactly like a link does.
 */
export default function PendingInvites() {
  const { token } = useAuth();
  const { acceptOrg, acceptMeeting } = useInviteActions();
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
        error: response.ok ? "" : "could not load invites",
      });
    });
    return () => {
      cancelled = true;
    };
  }, [token, reloads]);

  const shown =
    result.stamp === stamp
      ? result
      : { orgInvites: [], meetingInvites: [], error: "" };
  const { orgInvites, meetingInvites, error } = shown;

  function refresh() {
    setReloads((count) => count + 1);
  }

  /// Every button here is the same shape: act, then re-read the list.
  async function act(invite, action) {
    setBusyId(invite.id);
    await action();
    setBusyId(null);
    refresh();
  }

  function decline(kind, invite) {
    return callApi({
      method: "POST",
      path: `/private/invites/${kind}/${encodeURIComponent(invite.id)}/decline`,
      token,
    });
  }

  // Silent when there's nothing to show — a fetch error here shouldn't
  // take a whole page down with it.
  if (error || (orgInvites.length === 0 && meetingInvites.length === 0)) return null;

  return (
    <Card className="border-warning/50">
      <CardHeader title="invites" subtitle="waiting for you" />
      <ul className="grid gap-2">
        {orgInvites.map((invite) => (
          <li
            key={invite.id}
            className="flex flex-wrap items-center gap-2 rounded-lg bg-muted px-3 py-2"
          >
            <span className="min-w-0 flex-1 truncate text-sm text-ink">
              {invite.invited_by} invited you to{" "}
              <strong className="font-semibold">{invite.org_name}</strong>
            </span>
            <Button
              size="sm"
              disabled={busyId === invite.id}
              onClick={() => act(invite, () => acceptOrg(invite.id))}
            >
              accept
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={busyId === invite.id}
              onClick={() => act(invite, () => decline("org", invite))}
            >
              decline
            </Button>
          </li>
        ))}
        {meetingInvites.map((invite) => (
          <li
            key={invite.id}
            className="flex flex-wrap items-center gap-2 rounded-lg bg-muted px-3 py-2"
          >
            <span className="min-w-0 flex-1 truncate text-sm text-ink">
              {invite.invited_by} invited you to a meeting
            </span>
            <Button
              size="sm"
              disabled={busyId === invite.id}
              onClick={() => act(invite, () => acceptMeeting(invite.id))}
            >
              join
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={busyId === invite.id}
              onClick={() => act(invite, () => decline("meetings", invite))}
            >
              decline
            </Button>
          </li>
        ))}
      </ul>
    </Card>
  );
}
