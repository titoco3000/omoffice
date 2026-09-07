import { useEffect, useState } from "react";
import { callApi, describeFailure } from "../api";

/**
 * Every meeting in `orgId`, from `GET /private/meetings?org_id=...` -- not
 * just ones the caller made or joined: a meeting is a shared org resource,
 * so any member can rejoin any of them (see `backend/src/meetings/mod.rs`).
 *
 * This is the one thing on the page that comes out of the database rather than
 * out of Chime: Chime forgets a meeting the moment it empties, so without the
 * `meetings` table there is nothing to list. A code whose meeting has ended is
 * still shown — rejoining it answers `404 meeting_not_found`, which is the
 * honest result.
 *
 * `stamp` is which fetch the held result belongs to, so "loading" is derived
 * from the state rather than set alongside it, and a token or org change
 * counts as a new fetch just like pressing refresh.
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
    <div className="mt-4 border-t border-slate-800 pt-3">
      <div className="flex items-center gap-2">
        <h3 className="text-xs font-medium text-slate-400">org meetings</h3>
        <button
          type="button"
          onClick={() => setReloads((count) => count + 1)}
          disabled={!token || !orgId || loading}
          className="text-xs text-slate-500 hover:text-slate-300 disabled:opacity-50"
        >
          {loading ? "..." : "refresh"}
        </button>
      </div>

      {shown.error && (
        <p className="mt-2 font-mono text-xs break-all text-rose-300">
          {shown.error}
        </p>
      )}

      {!shown.error && shown.meetings.length === 0 && (
        <p className="mt-2 text-xs text-slate-600">
          {!orgId
            ? "pick an org above to see its meetings"
            : loading
              ? "loading..."
              : "none yet"}
        </p>
      )}

      <ul className="mt-2 grid gap-1">
        {shown.meetings.map((meeting) => (
          <li
            key={meeting.code}
            className="flex items-center gap-3 rounded-lg bg-slate-950 px-2 py-1.5"
          >
            <code className="min-w-0 flex-1 truncate font-mono text-xs text-slate-300">
              {meeting.code}
            </code>
            <span className="shrink-0 text-xs text-slate-500">
              {meeting.created_by}
            </span>
            <span className="shrink-0 text-xs text-slate-600">
              {new Date(meeting.created_at).toLocaleString()}
            </span>
            <button
              type="button"
              onClick={() => onPick(meeting.code)}
              className="shrink-0 rounded bg-slate-700 px-2 py-0.5 text-xs text-white hover:bg-slate-600"
            >
              rejoin
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
