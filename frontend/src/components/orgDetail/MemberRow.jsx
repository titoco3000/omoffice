import { useState } from "react";
import { callApi, describeFailure } from "../../api";
import { visibleRoles } from "../../rbac";
import RoleCheckboxes from "../RoleCheckboxes";

/// One member row, with an inline role editor for `team-management` holders.
export default function MemberRow({ member, you, canManage, org, token, onChanged }) {
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
