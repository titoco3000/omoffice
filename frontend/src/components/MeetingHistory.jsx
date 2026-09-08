import { useEffect, useState } from "react";
import { callApi, describeFailure } from "../api";
import Button from "../ui/Button";
import Notice from "../ui/Notice";

/**
 * Every meeting in `orgId`, not just ones the caller made or joined. A code
 * whose meeting has ended still shows; rejoining just 404s.
 */
export default function MeetingHistory({ token, orgId, onPick }) {
  const [reloads, setReloads] = useState(0);
  const [result, setResult] = useState({ stamp: null, meetings: [], error: "" });

  const stamp = `${token}:${orgId}:${reloads}`;
  const loading = Boolean(token && orgId) && result.stamp !== stamp;

  useEffect(() => {
    if (!token || !orgId) return undefined;
    let cancelled = false;

    callApi({
      path: `/private/meetings?org_id=${encodeURIComponent(orgId)}`,
      token,
    }).then((response) => {
      if (cancelled) return;
      setResult({
        stamp: `${token}:${orgId}:${reloads}`,
        meetings: response.ok ? (response.data.meetings ?? []) : [],
        error: response.ok ? "" : describeFailure(response),
      });
    });

    return () => {
      cancelled = true;
    };
  }, [token, orgId, reloads]);

  const shown = result.stamp === stamp ? result : { meetings: [], error: "" };

  return (
    <div className="grid gap-2">
      {shown.error && <Notice>{shown.error}</Notice>}

      {!shown.error && shown.meetings.length === 0 && (
        <p className="text-xs text-ink-faint">
          {!orgId ? "no org selected" : loading ? "loading..." : "none yet"}
        </p>
      )}

      <ul className="grid gap-1">
        {shown.meetings.map((meeting) => (
          <li
            key={meeting.code}
            className="flex flex-wrap items-center gap-3 rounded-lg bg-muted px-3 py-2"
          >
            <code className="min-w-0 flex-1 truncate font-mono text-xs text-ink">
              {meeting.code}
            </code>
            <span className="shrink-0 text-xs text-ink-muted">
              {meeting.created_by}
            </span>
            <span className="shrink-0 text-xs text-ink-faint">
              {new Date(meeting.created_at).toLocaleString()}
            </span>
            <Button variant="secondary" size="sm" onClick={() => onPick(meeting.code)}>
              rejoin
            </Button>
          </li>
        ))}
      </ul>

      <Button
        variant="ghost"
        size="sm"
        className="justify-self-start"
        onClick={() => setReloads((count) => count + 1)}
        disabled={!token || !orgId || loading}
      >
        {loading ? "..." : "refresh"}
      </Button>
    </div>
  );
}
