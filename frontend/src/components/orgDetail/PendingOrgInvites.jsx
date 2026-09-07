import { useEffect, useState } from "react";
import { callApi, describeFailure } from "../../api";
import { visibleRoles } from "../../rbac";

/**
 * The org's still-pending invites, team-management only, with a revoke
 * button per row. Own "stamp" pattern, same as `OrgDetail`'s roster fetch.
 */
export default function PendingOrgInvites({ org, token }) {
  const [reloads, setReloads] = useState(0);
  const [result, setResult] = useState({ stamp: null, invites: [], error: "" });
  const [revokingId, setRevokingId] = useState(null);

  const stamp = `${org.id}:${token}:${reloads}`;
  useEffect(() => {
    let cancelled = false;
    callApi({
      path: `/private/orgs/${encodeURIComponent(org.id)}/invites`,
      token,
    }).then((response) => {
      if (cancelled) return;
      setResult({
        stamp: `${org.id}:${token}:${reloads}`,
        invites: response.ok ? (response.data.invites ?? []) : [],
        error: response.ok ? "" : describeFailure(response),
      });
    });
    return () => {
      cancelled = true;
    };
  }, [org.id, token, reloads]);

  const shown = result.stamp === stamp ? result : { invites: [], error: "" };
  if (shown.error || shown.invites.length === 0) return null;

  async function revoke(invite) {
    setRevokingId(invite.id);
    await callApi({
      method: "DELETE",
      path: `/private/orgs/${encodeURIComponent(org.id)}/invites/${encodeURIComponent(invite.id)}`,
      token,
    });
    setRevokingId(null);
    setReloads((count) => count + 1);
  }

  return (
    <div className="mt-3 border-t border-slate-800 pt-3">
      <h3 className="text-xs font-medium text-slate-400">pending invites</h3>
      <ul className="mt-2 grid gap-1">
        {shown.invites.map((invite) => (
          <li
            key={invite.id}
            className="flex flex-wrap items-center gap-2 rounded-lg bg-slate-950 px-3 py-2"
          >
            <span className="min-w-0 flex-1 truncate font-mono text-sm text-slate-300">
              {invite.email}
            </span>
            <span className="flex shrink-0 flex-wrap gap-1">
              {visibleRoles(invite.roles).map((role) => (
                <span
                  key={role}
                  className="rounded bg-sky-500/15 px-1.5 py-0.5 text-xs text-sky-300"
                >
                  {role}
                </span>
              ))}
            </span>
            <button
              type="button"
              disabled={revokingId === invite.id}
              onClick={() => revoke(invite)}
              className="shrink-0 rounded-lg bg-slate-700 px-2 py-1 text-xs text-white hover:bg-slate-600 disabled:opacity-50"
            >
              revoke
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
