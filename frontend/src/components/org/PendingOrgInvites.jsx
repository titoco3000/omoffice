import { useEffect, useState } from "react";
import { callApi } from "../../api";
import { visibleRoles } from "../../rbac";
import Badge from "../../ui/Badge";
import Button from "../../ui/Button";

/**
 * The org's still-pending invites, team-management only, with a revoke
 * button per row. Own "stamp" pattern, same as the roster fetch above it.
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
        error: response.ok ? "" : "could not load pending invites",
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
    <div>
      <h3 className="mb-2 text-xs font-semibold tracking-wide text-ink-faint uppercase">
        pending invites
      </h3>
      <ul className="grid gap-1">
        {shown.invites.map((invite) => (
          <li
            key={invite.id}
            className="flex flex-wrap items-center gap-2 rounded-lg bg-muted px-3 py-2"
          >
            <span className="min-w-0 flex-1 truncate font-mono text-sm text-ink-muted">
              {invite.email}
            </span>
            <span className="flex shrink-0 flex-wrap gap-1">
              {visibleRoles(invite.roles).map((role) => (
                <Badge key={role} tone="brand">
                  {role}
                </Badge>
              ))}
            </span>
            <Button
              variant="ghost"
              size="sm"
              disabled={revokingId === invite.id}
              onClick={() => revoke(invite)}
            >
              revoke
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}
