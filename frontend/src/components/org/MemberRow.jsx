import { useState } from "react";
import { callApi, describeFailure } from "../../api";
import { visibleRoles } from "../../rbac";
import Avatar from "../../ui/Avatar";
import Badge from "../../ui/Badge";
import Button from "../../ui/Button";
import Notice from "../../ui/Notice";
import RoleCheckboxes from "./RoleCheckboxes";

/// One row of the org's member table, with an inline role editor for
/// `team-management` holders.
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
    <li className="border-t border-line px-3 py-2.5 first:border-t-0">
      <div className="flex flex-wrap items-center gap-2">
        <Avatar email={member.email} size="sm" />
        <span className="min-w-0 flex-1 truncate font-mono text-sm text-ink">
          {member.email}
          {you && <span className="ml-1 text-ink-faint">(you)</span>}
        </span>
        {!editing && (
          <span className="flex shrink-0 flex-wrap gap-1">
            {visibleRoles(member.roles).map((role) => (
              <Badge key={role} tone="brand">
                {role}
              </Badge>
            ))}
          </span>
        )}
        {canManage && !editing && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setRoles(visibleRoles(member.roles));
              setEditing(true);
            }}
          >
            edit roles
          </Button>
        )}
      </div>

      {editing && (
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <RoleCheckboxes roles={roles} onChange={setRoles} />
          <Button
            size="sm"
            variant={roles.length === 0 ? "danger" : "primary"}
            onClick={save}
            disabled={saving}
          >
            {saving ? "..." : roles.length === 0 ? "clear roles" : "save"}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setEditing(false)}>
            cancel
          </Button>
          {error && <Notice className="w-full">{error}</Notice>}
        </div>
      )}
    </li>
  );
}
