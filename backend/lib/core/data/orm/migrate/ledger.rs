//! The ledger row: `schema_migrations`'s shape in Rust, and reading the
//! whole table at once.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;

#[derive(sqlx::FromRow)]
pub(super) struct LedgerRow {
    pub(super) version: String,
    pub(super) name: String,
    pub(super) checksum: String,
    pub(super) status: String,
    pub(super) applied_at: Option<DateTime<Utc>>,
}

/// Every row, keyed by version.
pub(super) async fn read_ledger(pool: &PgPool) -> Result<BTreeMap<String, LedgerRow>, String> {
    let rows: Vec<LedgerRow> =
        sqlx::query_as("SELECT version, name, checksum, status, applied_at FROM schema_migrations")
            .fetch_all(pool)
            .await
            .map_err(|err| format!("could not read the schema_migrations ledger: {err}"))?;

    Ok(rows
        .into_iter()
        .map(|row| (row.version.clone(), row))
        .collect())
}
