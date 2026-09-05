//! Low-level building blocks: talking to DSQL, talking to Chime, describing a
//! table's shape as data. Nothing in here knows about a user or a meeting —
//! `users/`, `meetings/` and `schema.rs` are the ones that put these together
//! into a feature.

pub mod chime;
pub mod db;
pub mod orm;
