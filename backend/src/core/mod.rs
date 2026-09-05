//! Low-level building blocks: talking to DSQL, talking to Chime, talking to
//! API Gateway's Management API, describing a table's shape as data. Nothing
//! in here knows about a user, a meeting, or a chat room — `users/`,
//! `meetings/`, `chat/` and `schema.rs` are the ones that put these together
//! into a feature.

pub mod apigw_management;
pub mod chime;
pub mod db;
pub mod orm;
