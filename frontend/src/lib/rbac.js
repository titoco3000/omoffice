//! Calls into `/rbac/*`, the contract `core::rbac::routes` mounts
//! (backend/lib/core/rbac/mod.rs). Wiring that router is tracked as its own
//! phase (14.4) -- these calls are the shape it's expected to grow into.

import { apiFetch } from "./api";

export const requestKey = (email) => apiFetch(`/auth/request-key/${encodeURIComponent(email)}`);

export const me = (token) => apiFetch("/rbac/me", { token });
export const renameMe = (token, name) =>
  apiFetch("/rbac/me/name", { method: "PUT", token, body: { name } });

export const listOrgs = (token) => apiFetch("/rbac/orgs", { token });
export const createOrg = (token, name) =>
  apiFetch("/rbac/orgs", { method: "POST", token, body: { name } });
export const renameOrg = (token, orgId, name) =>
  apiFetch(`/rbac/orgs/${orgId}/name`, { method: "PUT", token, body: { name } });
export const deleteOrg = (token, orgId) => apiFetch(`/rbac/orgs/${orgId}`, { method: "DELETE", token });
export const leaveOrg = (token, orgId) =>
  apiFetch(`/rbac/orgs/${orgId}/members/me`, { method: "DELETE", token });

export const listMembers = (token, orgId) => apiFetch(`/rbac/orgs/${orgId}/members`, { token });

export const assignRole = (token, orgId, userId, role) =>
  apiFetch(`/rbac/orgs/${orgId}/roles`, { method: "POST", token, body: { user_id: userId, role } });
export const revokeRole = (token, orgId, userId, role) =>
  apiFetch(`/rbac/orgs/${orgId}/roles`, { method: "DELETE", token, body: { user_id: userId, role } });

export const listInvites = (token, orgId) => apiFetch(`/rbac/orgs/${orgId}/invites`, { token });
export const createInvite = (token, orgId, email, roles) =>
  apiFetch(`/rbac/orgs/${orgId}/invites`, { method: "POST", token, body: { email, roles } });
export const revokeInvite = (token, orgId, inviteId) =>
  apiFetch(`/rbac/orgs/${orgId}/invites/${inviteId}`, { method: "DELETE", token });

export const getInvite = (token, inviteId) => apiFetch(`/rbac/invites/${inviteId}`, { token });
export const acceptInvite = (token, inviteId) =>
  apiFetch(`/rbac/invites/${inviteId}/accept`, { method: "POST", token });
export const declineInvite = (token, inviteId) =>
  apiFetch(`/rbac/invites/${inviteId}/decline`, { method: "POST", token });
