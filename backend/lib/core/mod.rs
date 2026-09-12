//! Low-level building blocks: the database handle, outbound/inbound
//! messaging, the Chime client, RBAC, and JWT auth. Nothing in here knows
//! about orgs, meetings or invites as product concepts, only as rows and
//! channel names.

pub mod auth;
pub mod chime;
pub mod communication;
pub mod data;
pub mod rbac;

/// Where the SPA is served, for a magic-link invite URL.
pub fn frontend_base_url() -> String {
    std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:5173".to_string())
}
