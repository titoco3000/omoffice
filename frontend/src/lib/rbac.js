//! Calls into `/rbac/*`, the contract `core::rbac::routes` mounts
//! (backend/lib/core/rbac/mod.rs). Wiring that router is tracked as its own
//! phase (14.4) -- these calls are the shape it's expected to grow into.

import { apiFetch } from "./api";

// Mirrors `core::rbac::CHANNEL` (backend/lib/core/rbac/mod.rs): pushed after
// a role change or membership change, `{org_id, members}` to the org's
// roster or `{org_id, removed_you: true}` to whoever just lost membership.
export const ORG_ROSTER_CHANNEL = "org_roster";

// Mirrors `core::rbac::ACTIVE_CHANNEL`: sent, not received -- tells the
// backend which org this connection currently means, `{org_id}` (or
// `{org_id: null}` for none), so `online` reflects the one org actually
// being looked at rather than every org this user belongs to.
export const ORG_ACTIVE_CHANNEL = "org_active";

export const requestKey = (email) =>
  apiFetch(`/auth/request-key/${encodeURIComponent(email)}`);

export const me = (token) => apiFetch("/rbac/me", { token });
export const renameMe = (token, name) =>
  apiFetch("/rbac/me/name", { method: "PUT", token, body: { name } });

export const listOrgs = (token) => apiFetch("/rbac/orgs", { token });
export const createOrg = (token, name) =>
  apiFetch("/rbac/orgs", { method: "POST", token, body: { name } });
export const renameOrg = (token, orgId, name) =>
  apiFetch(`/rbac/orgs/${orgId}/name`, {
    method: "PUT",
    token,
    body: { name },
  });
export const deleteOrg = (token, orgId) =>
  apiFetch(`/rbac/orgs/${orgId}`, { method: "DELETE", token });
export const leaveOrg = (token, orgId) =>
  apiFetch(`/rbac/orgs/${orgId}/members/me`, { method: "DELETE", token });

export const listMembers = (token, orgId) =>
  apiFetch(`/rbac/orgs/${orgId}/members`, { token });
export const removeMember = (token, orgId, userId) =>
  apiFetch(`/rbac/orgs/${orgId}/members/${userId}`, {
    method: "DELETE",
    token,
  });

export const assignRole = (token, orgId, userId, role) =>
  apiFetch(`/rbac/orgs/${orgId}/roles`, {
    method: "POST",
    token,
    body: { user_id: userId, role },
  });
export const revokeRole = (token, orgId, userId, role) =>
  apiFetch(`/rbac/orgs/${orgId}/roles`, {
    method: "DELETE",
    token,
    body: { user_id: userId, role },
  });

export const listInvites = (token, orgId) =>
  apiFetch(`/rbac/orgs/${orgId}/invites`, { token });
export const createInvite = (token, orgId, email, roles) =>
  apiFetch(`/rbac/orgs/${orgId}/invites`, {
    method: "POST",
    token,
    body: { email, roles },
  });
export const revokeInvite = (token, orgId, inviteId) =>
  apiFetch(`/rbac/orgs/${orgId}/invites/${inviteId}`, {
    method: "DELETE",
    token,
  });

// Every pending org invite addressed to the caller's own email, across
// every org -- covers invites issued while offline or before signup, not
// just the one riding along on the magic link that logged this tab in.
export const listMyInvites = (token) => apiFetch("/rbac/invites", { token });

export const getInvite = (token, inviteId) =>
  apiFetch(`/rbac/invites/${inviteId}`, { token });
export const acceptInvite = (token, inviteId) =>
  apiFetch(`/rbac/invites/${inviteId}/accept`, { method: "POST", token });
export const declineInvite = (token, inviteId) =>
  apiFetch(`/rbac/invites/${inviteId}/decline`, { method: "POST", token });
