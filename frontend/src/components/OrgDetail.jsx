import { useEffect, useState } from "react";
import { callApi, describeFailure } from "../api";
import { useAuth } from "../auth/AuthContext";
import { hasRole } from "../rbac";
import InviteForm from "./orgDetail/InviteForm";
import MemberRow from "./orgDetail/MemberRow";
import PendingOrgInvites from "./orgDetail/PendingOrgInvites";

/**
 * One org's roster. The backend enforces the last-admin rule; this just
 * surfaces the 409 it sends back.
 */
export default function OrgDetail({ org, token, onBack, onChanged }) {
  const { email: myEmail } = useAuth();
  const [notice, setNotice] = useState("");
  const [reloads, setReloads] = useState(0);
  // Same "stamp" pattern as `useOrgs`: loading is derived from whether the
  // held result matches the current fetch.
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
