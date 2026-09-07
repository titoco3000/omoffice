//! Low-level building blocks: talking to DSQL, talking to Chime, talking to
//! API Gateway's Management API, describing a table's shape as data. Nothing
//! in here knows about a user, a meeting, or a chat room — `users/`,
//! `meetings/`, `chat/` and `schema.rs` are the ones that put these together
//! into a feature.

pub mod apigw_management;
pub mod chime;
pub mod db;
pub mod orm;

/// `FRONTEND_URL` — where the SPA is served, for building a link back into
/// it (an invite's magic-link accept URL, `rbac.rs`/`meetings.rs`). Not
/// wired through terraform yet — `infra/make output <stage>`'s
/// `frontend_env_file` block would be the natural source for a real
/// deployment; this falls back to the local dev origin
/// `frontend/.env.local` already points `vite` at.
pub fn frontend_base_url() -> String {
    std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:5173".to_string())
}
