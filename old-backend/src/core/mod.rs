//! Low-level building blocks: DSQL, Chime, API Gateway management, outbound
//! messaging, schema-as-data. `users/`/`meetings/`/`chat/` own the features.

pub mod apigw_management;
pub mod chime;
pub mod db;
pub mod messenger;
pub mod orm;

/// Where the SPA is served, for an invite's magic-link URL. Not wired
/// through terraform yet; falls back to the local dev origin.
pub fn frontend_base_url() -> String {
    std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:5173".to_string())
}
