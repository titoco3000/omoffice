//! Describes a table's shape as Rust data, renders it to SQL, diffs two
//! shapes into a migration.

// Only `schema.rs`, `migrate.rs`'s `generate`, and this module's own tests
// use every item here, so plain `cargo build` sees some as unused.
#![allow(dead_code)]

pub mod diff;
pub mod schema_types;
pub mod state;

pub use diff::*;
pub use schema_types::*;
pub use state::*;
