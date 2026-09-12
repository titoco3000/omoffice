//! Everything schema-as-data and the database connection: [`db`] (the DSQL
//! handle) and [`orm`] (the table/column types `schema.rs` is built from,
//! the migration generator, and the runner that applies what it generates).

pub mod db;
pub mod orm;
