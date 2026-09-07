// `admin` also satisfies `team-management`/`billing`, mirroring `rbac::has_role`.
const IMPLIED_BY_ADMIN = ["team-management", "billing"];

// The backend's hidden default role (`rbac::roles::DEFAULT`) — bookkeeping,
// not a permission. Stripped wherever roles reach a person.
const HIDDEN_ROLE = "";

/// Does any org in `orgs` (`[{id, name, roles}]`) grant `role` for `orgId`?
/// Gate a piece of UI with this instead of letting the API reject the click.
export function hasRole(orgs, orgId, role) {
  const org = orgs.find((candidate) => candidate.id === orgId);
  if (!org) return false;
  if (org.roles.includes(role)) return true;
  return IMPLIED_BY_ADMIN.includes(role) && org.roles.includes("admin");
}

/// `roles` with the hidden default role removed — use wherever roles are
/// displayed or seeded into an editable set.
export function visibleRoles(roles) {
  return roles.filter((role) => role !== HIDDEN_ROLE);
}
