//! Calls into `/admin/*`, the sys-admin-only routes `core::rbac::sys_admin`
//! mounts (backend/lib/core/rbac/sys_admin.rs).

import { apiFetch } from "../../core/api";

// {sys_admins: [{user_id, email, name}, ...]}
export const listSysAdmins = (token) =>
  apiFetch("/admin/sys-admins", { token });
// Creates the user if there is none for `email`; answers with the new list.
export const addSysAdmin = (token, email) =>
  apiFetch("/admin/sys-admins", { method: "POST", token, body: { email } });
export const removeSysAdmin = (token, userId) =>
  apiFetch(`/admin/sys-admins/${userId}`, { method: "DELETE", token });

// {orgs: [{id, name, picture, user_count, online_count}, ...]}
export const listAdminOrgs = (token) => apiFetch("/admin/orgs", { token });
// {org: {id, name, picture, ...}} -- the roster is the regular `/rbac` one.
export const getAdminOrg = (token, orgId) =>
  apiFetch(`/admin/org/${orgId}`, { token });
