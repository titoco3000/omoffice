import { useState } from "react";
import { callApi, describeFailure } from "../api";
import { useOrgs } from "../hooks/useOrgs";
import { visibleRoles } from "../rbac";
import OrgDetail from "./OrgDetail";
import RoleCheckboxes from "./RoleCheckboxes";

/// The lobby for orgs: the list, a create-org form, and — once one is
/// picked — `OrgDetail` for its roster.
export default function Organizations({ token }) {
  const { orgs, loading, error, reload } = useOrgs(token);
  const [openId, setOpenId] = useState(null);
  const [creating, setCreating] = useState(false);

  const open = orgs.find((org) => org.id === openId);
  if (open) {
    return (
      <OrgDetail
        org={open}
        token={token}
        onBack={() => setOpenId(null)}
        onChanged={reload}
      />
    );
  }

  return (
    <section className="rounded-xl border border-slate-800 bg-slate-900/60 p-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-medium text-slate-300">organizations</h2>
        <button
          type="button"
          onClick={() => setCreating((value) => !value)}
          className="rounded-lg bg-sky-600 px-3 py-1 text-xs font-medium text-white hover:bg-sky-500"
        >
          {creating ? "cancel" : "new org"}
        </button>
      </div>

      {creating && (
        <CreateOrgForm
          token={token}
          onCreated={() => {
            setCreating(false);
            reload();
          }}
        />
      )}

      {error && (
        <p className="mt-3 font-mono text-xs break-all text-rose-300">{error}</p>
      )}
      {!error && orgs.length === 0 && (
        <p className="mt-3 text-xs text-slate-600">
          {loading ? "loading..." : "not a member of any org yet"}
        </p>
      )}

      <ul className="mt-3 grid gap-1">
        {orgs.map((org) => (
          <li key={org.id}>
            <button
              type="button"
              onClick={() => setOpenId(org.id)}
              className="flex w-full items-center gap-3 rounded-lg bg-slate-950 px-3 py-2 text-left hover:bg-slate-900"
            >
              <span className="min-w-0 flex-1 truncate text-sm text-slate-200">
                {org.name}
              </span>
              <span className="flex shrink-0 flex-wrap gap-1">
                {visibleRoles(org.roles).map((role) => (
                  <span
                    key={role}
                    className="rounded bg-sky-500/15 px-1.5 py-0.5 text-xs text-sky-300"
                  >
                    {role}
                  </span>
                ))}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function CreateOrgForm({ token, onCreated }) {
  const [name, setName] = useState("");
  const [invites, setInvites] = useState([]);
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  function addInviteRow() {
    setInvites((rows) => [...rows, { email: "", roles: [] }]);
  }

  function updateInviteRow(index, field, value) {
    setInvites((rows) =>
      rows.map((row, i) => (i === index ? { ...row, [field]: value } : row)),
    );
  }

  function removeInviteRow(index) {
    setInvites((rows) => rows.filter((_, i) => i !== index));
  }

  async function submit(event) {
    event.preventDefault();
    const wanted = name.trim();
    if (!wanted) return;
    setError("");
    setSubmitting(true);
    const response = await callApi({
      method: "POST",
      path: "/private/orgs",
      token,
      body: {
        name: wanted,
        // A row with no roles checked is still a real invite — the backend
        // folds in its own hidden default role.
        invites: invites.filter((row) => row.email.trim()),
      },
    });
    setSubmitting(false);
    if (!response.ok) {
      setError(describeFailure(response));
      return;
    }
    onCreated();
  }

  return (
    <form onSubmit={submit} className="mt-3 grid gap-2 border-t border-slate-800 pt-3">
      <input
        type="text"
        required
        value={name}
        onChange={(event) => setName(event.target.value)}
        placeholder="organization name"
        className="h-9 rounded-lg border border-slate-700 bg-slate-950 px-2 text-sm text-slate-100 outline-none focus:border-sky-500"
      />

      {invites.map((row, index) => (
        <div key={index} className="flex flex-wrap items-center gap-2 rounded-lg bg-slate-950 p-2">
          <input
            type="email"
            value={row.email}
            onChange={(event) => updateInviteRow(index, "email", event.target.value)}
            placeholder="invite@example.com"
            className="h-9 min-w-0 flex-1 rounded-lg border border-slate-700 bg-slate-950 px-2 font-mono text-sm text-slate-100 outline-none focus:border-sky-500"
          />
          <button
            type="button"
            onClick={() => removeInviteRow(index)}
            className="shrink-0 rounded-lg bg-slate-700 px-2 py-1.5 text-xs text-white hover:bg-slate-600"
          >
            remove
          </button>
          <RoleCheckboxes
            roles={row.roles}
            onChange={(roles) => updateInviteRow(index, "roles", roles)}
          />
        </div>
      ))}

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={addInviteRow}
          className="text-xs text-slate-400 hover:text-slate-200"
        >
          + invite someone
        </button>
        <button
          type="submit"
          disabled={submitting || !name.trim()}
          className="ml-auto rounded-lg bg-sky-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
        >
          {submitting ? "..." : "create"}
        </button>
      </div>

      {error && (
        <p className="rounded-lg border border-rose-800 bg-slate-950 p-3 font-mono text-xs break-all text-rose-300">
          {error}
        </p>
      )}
    </form>
  );
}
