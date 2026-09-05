// `admin` also satisfies a `team-management` or `billing` check, mirroring
// `rbac::has_role` on the backend: the role that can delete the org can also
// manage its membership and see its billing, so creating an org only ever
// grants its creator `admin` -- not all three native roles as separate rows.
// It never extends to a project-specific role (e.g. "post").
const IMPLIED_BY_ADMIN = ["team-management", "billing"];

// The backend's hidden default role (`rbac::roles::DEFAULT` in
// rbac/mod.rs -- the empty string): every member has it, but it's
// bookkeeping, not a permission -- nobody picks it or needs to see it.
// Stripped out wherever roles reach a person, not just the picker
// (`Organizations.jsx`'s `ROLE_OPTIONS`).
const HIDDEN_ROLE = "";

/// Does any org in `orgs` (the shape `GET /private/orgs` returns:
/// `[{id, name, roles}]`) grant `role` for `orgId`? The frontend counterpart
/// to the backend's `rbac::require_role` — gate a piece of UI with this
/// instead of showing it and letting the API reject the click.
export function hasRole(orgs, orgId, role) {
  const org = orgs.find((candidate) => candidate.id === orgId);
  if (!org) return false;
  if (org.roles.includes(role)) return true;
  return IMPLIED_BY_ADMIN.includes(role) && org.roles.includes("admin");
}

/// `roles` with the hidden default role removed — use this anywhere a
/// person's roles are displayed or seeded into an editable set, so it's
/// never shown and never something unchecking-everything has to fight past.
export function visibleRoles(roles) {
  return roles.filter((role) => role !== HIDDEN_ROLE);
}
