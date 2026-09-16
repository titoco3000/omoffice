pub mod auth;
pub mod chime;
pub mod communication;
pub mod data;
pub mod debug;
pub mod media;
pub mod rbac;

/// Where the SPA is served, for a magic-link invite URL.
pub fn frontend_base_url() -> String {
    std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:5173".to_string())
}
