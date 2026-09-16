//! Member table (name, email, roles, remove) plus the invite form -- the
//! `/org` page's people section. Requires `team-management` (or `admin`,
//! which implies it), enforced again on the backend regardless.

import { useEffect, useRef, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { useOrgSession } from "../org/OrgSessionContext";
import { ROLES } from "../lib/roles";
import { avatarUrl } from "../lib/avatars";
import AsyncButton from "./AsyncButton";
import Avatar from "./Avatar";
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
  // Optimistic role-checkbox state, keyed `${userId}:${role}` -> the value
  // shown while the backend call (and the reload after it) is in flight --
  // so a checkbox flips the instant it's clicked rather than waiting on the
  // round trip. Dropped once the real roster is known to agree with it (on
  // success, after `reloadMembers` resolves; on failure, immediately, since
  // nothing changed) -- not sooner, or the checkbox would flash back to the
  // old value for the gap in between.
  const [roleOverrides, setRoleOverrides] = useState({});
  // Enter-to-submit on the invite form routes through the same button
  // `AsyncButton` renders -- see the same trick in `pages/OrgNew.jsx`.
  const inviteButtonRef = useRef(null);

  useEffect(() => {
    listInvites(token, currentOrg.id)
      .then((data) => setInvites(data.invites ?? []))
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentOrg.id]);

  const clearRoleOverride = (key) =>
    setRoleOverrides((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });

  const toggleRole = async (userId, role, checked) => {
    const key = `${userId}:${role}`;
    setRoleOverrides((current) => ({ ...current, [key]: checked }));
    const call = checked ? assignRole : revokeRole;
    try {
      await call(token, currentOrg.id, userId, role);
      await reloadMembers();
    } catch (err) {
      setError(err.message);
    } finally {
      clearRoleOverride(key);
    }
  };

  const toggleInviteRole = (role) =>
    setInviteRoles((roles) =>
      roles.includes(role) ? roles.filter((r) => r !== role) : [...roles, role],
    );

  const preventImplicitSubmit = (event) => {
    event.preventDefault();
    inviteButtonRef.current?.click();
  };

  const submitInvite = async () => {
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

  const revoke = async (inviteId) => {
    try {
      await revokeInvite(token, currentOrg.id, inviteId);
      setInvites((list) => list.filter((invite) => invite.id !== inviteId));
    } catch (err) {
      setError(err.message);
    }
  };

  const kick = async (userId, displayName) => {
    if (!window.confirm(`Remove ${displayName} from ${currentOrg.name}?`))
      return;
    try {
      await removeMember(token, currentOrg.id, userId);
      await reloadMembers();
    } catch (err) {
      setError(err.message);
    }
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
                <div className="flex items-center gap-2">
                  <Avatar
                    url={avatarUrl(member.picture)}
                    name={member.display_name}
                    size="size-6"
                  />
                  <span
                    className={`inline-block ${member.online ? "text-success" : "text-ink-muted/40"}`}
                    title={member.online ? "online" : "offline"}
                  >
                    ●
                  </span>
                  {member.display_name}
                </div>
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
                      checked={
                        roleOverrides[`${member.user_id}:${role}`] ??
                        member.roles.includes(role)
                      }
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
                  <AsyncButton
                    onClick={() => kick(member.user_id, member.display_name)}
                    className="text-danger hover:underline"
                  >
                    remove
                  </AsyncButton>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <form
        onSubmit={preventImplicitSubmit}
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
        <AsyncButton
          ref={inviteButtonRef}
          onClick={submitInvite}
          className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong"
        >
          Invite
        </AsyncButton>
      </form>

      {invites.length > 0 && (
        <ul className="mt-3 space-y-1 text-sm">
          {invites.map((invite) => (
            <li
              key={invite.id}
              className="flex items-center justify-between text-ink-muted"
            >
              <span>{invite.email} -- pending</span>
              <AsyncButton
                onClick={() => revoke(invite.id)}
                className="text-danger hover:underline"
              >
                revoke
              </AsyncButton>
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
