//! Member table (name, email, roles, remove) plus the invite form -- the
//! `/org` page's people section. Requires `team-management` (or `admin`,
//! which implies it), enforced again on the backend regardless.

import { useEffect, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { useOrgSession } from "../org/OrgSessionContext";
import { ROLES } from "../lib/roles";
import {
  removeMember,
  createInvite,
  listInvites,
  revokeInvite,
  assignRole,
  revokeRole,
} from "../lib/rbac";

export default function UsersManagement() {
  const { token } = useAuth();
  const { myId, currentOrg, isAdmin, roster } = useOrgSession();
  const { members, reload: reloadMembers, error: rosterError } = roster;
  const [invites, setInvites] = useState([]);
  const [inviteEmail, setInviteEmail] = useState("");
  const [inviteRoles, setInviteRoles] = useState([]);
  const [error, setError] = useState(null);

  useEffect(() => {
    listInvites(token, currentOrg.id)
      .then((data) => setInvites(data.invites ?? []))
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentOrg.id]);

  const toggleRole = (userId, role, checked) => {
    const call = checked ? assignRole : revokeRole;
    call(token, currentOrg.id, userId, role)
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
      await createInvite(token, currentOrg.id, inviteEmail.trim(), inviteRoles);
      setInviteEmail("");
      setInviteRoles([]);
      const data = await listInvites(token, currentOrg.id);
      setInvites(data.invites ?? []);
    } catch (err) {
      setError(err.message);
    }
  };

  const revoke = (inviteId) =>
    revokeInvite(token, currentOrg.id, inviteId)
      .then(() =>
        setInvites((list) => list.filter((invite) => invite.id !== inviteId)),
      )
      .catch((err) => setError(err.message));

  const kick = (userId, displayName) => {
    if (!window.confirm(`Remove ${displayName} from ${currentOrg.name}?`))
      return;
    removeMember(token, currentOrg.id, userId)
      .then(reloadMembers)
      .catch((err) => setError(err.message));
  };

  return (
    <section>
      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-ink-muted">
            <th className="py-1 font-medium">Member</th>
            <th className="py-1 font-medium">Email</th>
            <th className="py-1 font-medium">Roles</th>
            <th className="py-1" />
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
              <td className="py-1.5 text-ink-muted">{member.email ?? "--"}</td>
              <td className="py-1.5">
                {ROLES.map((role) => (
                  <label
                    key={role}
                    className="mr-3 inline-flex items-center gap-1 text-ink-muted"
                  >
                    <input
                      type="checkbox"
                      checked={member.roles.includes(role)}
                      onChange={(event) =>
                        toggleRole(member.user_id, role, event.target.checked)
                      }
                    />
                    {role}
                  </label>
                ))}
              </td>
              <td className="py-1.5 text-right">
                {member.user_id !== myId && (
                  <button
                    onClick={() => kick(member.user_id, member.display_name)}
                    className="text-danger hover:underline"
                  >
                    remove
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>

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

      {(error || rosterError) && (
        <p className="mt-3 text-sm text-danger">{error || rosterError}</p>
      )}
      {!isAdmin && (
        <p className="mt-3 text-xs text-ink-faint">
          Only admins may change another admin's roles.
        </p>
      )}
    </section>
  );
}
