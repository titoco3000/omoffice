import { useEffect, useState } from "react";
import { decodeJwt } from "../lib/jwt";
import { ROLES } from "../lib/roles";
import {
  removeMember,
  renameOrg,
  deleteOrg,
  createInvite,
  listInvites,
  revokeInvite,
  assignRole,
  revokeRole,
} from "../lib/rbac";

export default function OrgPanel({
  token,
  org,
  roster,
  onOrgChanged,
  onOrgDeleted,
}) {
  const myId = decodeJwt(token)?.sub;
  const { members, reload: reloadMembers, error: rosterError } = roster;
  const [invites, setInvites] = useState([]);
  const [name, setName] = useState(org.name);
  const [inviteEmail, setInviteEmail] = useState("");
  const [inviteRoles, setInviteRoles] = useState([]);
  const [error, setError] = useState(null);

  const me = members.find((member) => member.user_id === myId);
  const isAdmin = me?.roles.includes("admin") ?? false;
  const canManageTeam =
    isAdmin || (me?.roles.includes("team-management") ?? false);

  useEffect(() => {
    if (!canManageTeam) return;
    listInvites(token, org.id)
      .then((data) => setInvites(data.invites ?? []))
      .catch(() => {});
  }, [token, org.id, canManageTeam]);

  const saveName = () => {
    if (!name.trim() || name === org.name) return;
    renameOrg(token, org.id, name.trim())
      .then(onOrgChanged)
      .catch((err) => setError(err.message));
  };

  const toggleRole = (userId, role, checked) => {
    const call = checked ? assignRole : revokeRole;
    call(token, org.id, userId, role)
      .then(reloadMembers)
      .catch((err) => setError(err.message));
  };

  const toggleInviteRole = (role) =>
    setInviteRoles((roles) =>
      roles.includes(role) ? roles.filter((r) => r !== role) : [...roles, role],
    );

  const submitInvite = async (event) => {
    event.preventDefault();
    try {
      await createInvite(token, org.id, inviteEmail.trim(), inviteRoles);
      setInviteEmail("");
      setInviteRoles([]);
      const data = await listInvites(token, org.id);
      setInvites(data.invites ?? []);
    } catch (err) {
      setError(err.message);
    }
  };

  const revoke = (inviteId) =>
    revokeInvite(token, org.id, inviteId)
      .then(() =>
        setInvites((list) => list.filter((invite) => invite.id !== inviteId)),
      )
      .catch((err) => setError(err.message));

  const removeMemberFromOrg = (userId, displayName) => {
    if (!window.confirm(`Remove ${displayName} from ${org.name}?`)) return;
    removeMember(token, org.id, userId)
      .then(reloadMembers)
      .catch((err) => setError(err.message));
  };

  const removeOrg = () => {
    if (!window.confirm(`Delete ${org.name}? This cannot be undone.`)) return;
    deleteOrg(token, org.id)
      .then(onOrgDeleted)
      .catch((err) => setError(err.message));
  };

  return (
    <section className="rounded-card border bg-surface p-5">
      {isAdmin ? (
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          onBlur={saveName}
          className="rounded-card border bg-bg px-3 py-1.5 text-lg font-semibold text-ink"
        />
      ) : (
        <h2 className="text-lg font-semibold text-ink">{org.name}</h2>
      )}

      <table className="mt-4 w-full text-sm">
        <thead>
          <tr className="text-left text-ink-muted">
            <th className="py-1 font-medium">Member</th>
            <th className="py-1 font-medium">Roles</th>
            {canManageTeam && <th className="py-1" />}
          </tr>
        </thead>
        <tbody>
          {members.map((member) => (
            <tr key={member.user_id} className="border-t">
              <td className="py-1.5 text-ink">
                <span
                  className={`mr-1.5 inline-block ${member.online ? "text-success" : "text-ink-muted/40"}`}
                  title={member.online ? "online" : "offline"}
                >
                  ●
                </span>
                {member.display_name}
              </td>
              <td className="py-1.5">
                {ROLES.map((role) => (
                  <label
                    key={role}
                    className="mr-3 inline-flex items-center gap-1 text-ink-muted"
                  >
                    <input
                      type="checkbox"
                      disabled={!canManageTeam}
                      checked={member.roles.includes(role)}
                      onChange={(event) =>
                        toggleRole(member.user_id, role, event.target.checked)
                      }
                    />
                    {role}
                  </label>
                ))}
              </td>
              {canManageTeam && (
                <td className="py-1.5 text-right">
                  {member.user_id !== myId && (
                    <button
                      onClick={() =>
                        removeMemberFromOrg(member.user_id, member.display_name)
                      }
                      className="text-danger hover:underline"
                    >
                      remove
                    </button>
                  )}
                </td>
              )}
            </tr>
          ))}
        </tbody>
      </table>

      {canManageTeam && (
        <>
          <form
            onSubmit={submitInvite}
            className="mt-4 flex flex-wrap items-center gap-2"
          >
            <input
              type="email"
              required
              value={inviteEmail}
              onChange={(event) => setInviteEmail(event.target.value)}
              placeholder="invite@example.com"
              className="flex-1 rounded-card border bg-bg px-3 py-1.5 text-ink"
            />
            {ROLES.map((role) => (
              <label
                key={role}
                className="inline-flex items-center gap-1 text-sm text-ink-muted"
              >
                <input
                  type="checkbox"
                  checked={inviteRoles.includes(role)}
                  onChange={() => toggleInviteRole(role)}
                />
                {role}
              </label>
            ))}
            <button className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong">
              Invite
            </button>
          </form>

          {invites.length > 0 && (
            <ul className="mt-3 space-y-1 text-sm">
              {invites.map((invite) => (
                <li
                  key={invite.id}
                  className="flex items-center justify-between text-ink-muted"
                >
                  <span>{invite.email} -- pending</span>
                  <button
                    onClick={() => revoke(invite.id)}
                    className="text-danger hover:underline"
                  >
                    revoke
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      {isAdmin && (
        <div className="mt-5 rounded-card border border-danger/40 p-3">
          <p className="text-sm text-ink-muted">
            Deletes the org for every member. Cannot be undone.
          </p>
          <button
            onClick={removeOrg}
            className="mt-2 text-sm font-medium text-danger hover:underline"
          >
            Delete organization
          </button>
        </div>
      )}

      {(error || rosterError) && (
        <p className="mt-3 text-sm text-danger">{error || rosterError}</p>
      )}
    </section>
  );
}
