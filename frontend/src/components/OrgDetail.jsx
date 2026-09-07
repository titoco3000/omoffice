import { useEffect, useState } from "react";
import { callApi, describeFailure } from "../api";
import { useAuth } from "../auth/AuthContext";
import { hasRole, visibleRoles } from "../rbac";
import { RoleCheckboxes } from "./Organizations";

/**
 * One org's roster. `org` is the summary `Organizations` already has
 * (`{id, name, roles}` — the caller's own roles), which is enough to gate the
 * invite form and the delete button without waiting on a fetch; the roster
 * itself still needs `GET /private/orgs/{id}`.
 *
 * The invite form and the per-member role editor both need `team-management`;
 * deleting the org needs `admin`; leaving needs nothing beyond being a
 * member, and the backend is what actually enforces the last-admin rule --
 * this only surfaces the 409 it sends back.
 */
export default function OrgDetail({ org, token, onBack, onChanged }) {
  const { email: myEmail } = useAuth();
  const [notice, setNotice] = useState("");
  const [reloads, setReloads] = useState(0);
  // Same "stamp" pattern as `useOrgs`/`MeetingHistory`: loading is derived
  // from whether the held result matches the current fetch, not tracked
  // alongside it.
  const [result, setResult] = useState({ stamp: null, members: [], error: "" });

  const canManage = hasRole([org], org.id, "team-management");
  const canDelete = hasRole([org], org.id, "admin");

  const stamp = `${org.id}:${token}:${reloads}`;
  const loading = result.stamp !== stamp;

  useEffect(() => {
    let cancelled = false;
    callApi({ path: `/private/orgs/${encodeURIComponent(org.id)}`, token }).then(
      (response) => {
        if (cancelled) return;
        setResult({
          stamp: `${org.id}:${token}:${reloads}`,
          members: response.ok ? (response.data.members ?? []) : [],
          error: response.ok ? "" : describeFailure(response),
        });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [org.id, token, reloads]);

  const shown = result.stamp === stamp ? result : { members: [], error: "" };
  const members = shown.members;
  const error = shown.error;

  function refresh() {
    setReloads((count) => count + 1);
  }

  async function leave() {
    setNotice("");
    const response = await callApi({
      method: "DELETE",
      path: `/private/orgs/${encodeURIComponent(org.id)}/members/me`,
      token,
    });
    if (!response.ok) {
      setNotice(describeFailure(response));
      return;
    }
    onChanged();
    onBack();
  }

  async function deleteOrg() {
    setNotice("");
    const response = await callApi({
      method: "DELETE",
      path: `/private/orgs/${encodeURIComponent(org.id)}`,
      token,
    });
    if (!response.ok) {
      setNotice(describeFailure(response));
      return;
    }
    onChanged();
    onBack();
  }

  return (
    <section className="rounded-xl border border-slate-800 bg-slate-900/60 p-4">
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onBack}
          className="text-xs text-slate-400 hover:text-slate-200"
        >
          ← orgs
        </button>
        <h2 className="min-w-0 flex-1 truncate text-sm font-medium text-slate-300">
          {org.name}
        </h2>
        <button
          type="button"
          onClick={leave}
          className="rounded-lg bg-slate-700 px-3 py-1 text-xs text-white hover:bg-slate-600"
        >
          leave
        </button>
        {canDelete && (
          <button
            type="button"
            onClick={deleteOrg}
            className="rounded-lg bg-rose-700 px-3 py-1 text-xs text-white hover:bg-rose-600"
          >
            delete org
          </button>
        )}
      </div>

      {notice && (
        <p className="mt-3 rounded-lg border border-rose-800 bg-slate-950 p-3 font-mono text-xs break-all text-rose-300">
          {notice}
        </p>
      )}

      {error && <p className="mt-3 font-mono text-xs break-all text-rose-300">{error}</p>}
      {!error && loading && <p className="mt-3 text-xs text-slate-600">loading...</p>}

      <ul className="mt-3 grid gap-1">
        {members.map((member) => (
          <MemberRow
            key={member.email}
            member={member}
            you={member.email === myEmail}
            canManage={canManage}
            org={org}
            token={token}
            onChanged={refresh}
          />
        ))}
      </ul>

      {canManage && <InviteForm org={org} token={token} onInvited={refresh} />}
      {canManage && <PendingOrgInvites org={org} token={token} />}
    </section>
  );
}

/**
 * The org's still-pending invites (`GET /private/orgs/{org_id}/invites`),
 * team-management only, with a revoke button per row. Own "stamp" pattern,
 * same as `OrgDetail`'s roster fetch just above -- kept separate rather than
 * folded into it since the two lists (members, pending invites) come from
 * different routes and refresh independently.
 */
function PendingOrgInvites({ org, token }) {
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

function MemberRow({ member, you, canManage, org, token, onChanged }) {
  const [editing, setEditing] = useState(false);
  const [roles, setRoles] = useState(() => visibleRoles(member.roles));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function save() {
    setSaving(true);
    setError("");
    const response = await callApi({
      method: "PUT",
      path: `/private/orgs/${encodeURIComponent(org.id)}/members/${encodeURIComponent(member.email)}/roles`,
      token,
      body: { roles },
    });
    setSaving(false);
    if (!response.ok) {
      setError(describeFailure(response));
      return;
    }
    setEditing(false);
    onChanged();
  }

  return (
    <li className="rounded-lg bg-slate-950 px-3 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="min-w-0 flex-1 truncate font-mono text-sm text-slate-200">
          {member.email}
          {you && <span className="ml-1 text-slate-500">(you)</span>}
        </span>
        {!editing &&
          visibleRoles(member.roles).map((role) => (
            <span
              key={role}
              className="rounded bg-sky-500/15 px-1.5 py-0.5 text-xs text-sky-300"
            >
              {role}
            </span>
          ))}
        {canManage && !editing && (
          <button
            type="button"
            onClick={() => {
              setRoles(visibleRoles(member.roles));
              setEditing(true);
            }}
            className="shrink-0 text-xs text-slate-400 hover:text-slate-200"
          >
            edit roles
          </button>
        )}
      </div>

      {editing && (
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <RoleCheckboxes roles={roles} onChange={setRoles} />
          <button
            type="button"
            onClick={save}
            disabled={saving}
            className={`rounded-lg px-3 py-1 text-xs font-medium text-white disabled:opacity-50 ${
              roles.length === 0 ? "bg-amber-700 hover:bg-amber-600" : "bg-sky-600 hover:bg-sky-500"
            }`}
          >
            {saving ? "..." : roles.length === 0 ? "clear roles" : "save"}
          </button>
          <button
            type="button"
            onClick={() => setEditing(false)}
            className="text-xs text-slate-400 hover:text-slate-200"
          >
            cancel
          </button>
          {error && <p className="w-full font-mono text-xs break-all text-rose-300">{error}</p>}
        </div>
      )}
    </li>
  );
}

function InviteForm({ org, token, onInvited }) {
  const [email, setEmail] = useState("");
  // No roles checked is a real choice, not an empty form: the backend folds
  // in its hidden default role, so this invites a plain member.
  const [roles, setRoles] = useState([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  async function submit(event) {
    event.preventDefault();
    const wanted = email.trim();
    if (!wanted) return;
    setError("");
    setSubmitting(true);
    // A pending invite, not instant membership -- the email gets a magic
    // link (or a toast if they're already logged in) and only actually
    // joins once they accept it.
    const response = await callApi({
      method: "POST",
      path: `/private/orgs/${encodeURIComponent(org.id)}/invites`,
      token,
      body: { email: wanted, roles },
    });
    setSubmitting(false);
    if (!response.ok) {
      setError(describeFailure(response));
      return;
    }
    setEmail("");
    onInvited();
  }

  return (
    <form
      onSubmit={submit}
      className="mt-3 flex flex-wrap items-center gap-3 border-t border-slate-800 pt-3"
    >
      <input
        type="email"
        required
        value={email}
        onChange={(event) => setEmail(event.target.value)}
        placeholder="invite@example.com"
        className="h-9 min-w-0 flex-1 rounded-lg border border-slate-700 bg-slate-950 px-2 font-mono text-sm text-slate-100 outline-none focus:border-sky-500"
      />
      <RoleCheckboxes roles={roles} onChange={setRoles} />
      <button
        type="submit"
        disabled={submitting || !email.trim()}
        className="rounded-lg bg-sky-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
      >
        {submitting ? "..." : "invite"}
      </button>
      {error && (
        <p className="w-full font-mono text-xs break-all text-rose-300">{error}</p>
      )}
    </form>
  );
}
