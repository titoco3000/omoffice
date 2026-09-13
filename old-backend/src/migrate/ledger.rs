//! Reading `schema_migrations` back into a `BTreeMap` keyed by version.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::Row;

pub(super) struct LedgerRow {
    pub(super) version: String,
    pub(super) name: String,
    pub(super) checksum: String,
    pub(super) status: String,
    pub(super) applied_at: Option<DateTime<Utc>>,
}

/// Keyed by version, so iteration is in migration order.
pub(super) async fn read_ledger(pool: &PgPool) -> Result<BTreeMap<String, LedgerRow>, String> {
    let rows =
        sqlx::query("SELECT version, name, checksum, status, applied_at FROM schema_migrations")
            .fetch_all(pool)
            .await
            .map_err(|err| format!("could not read schema_migrations: {err}"))?;

    rows.into_iter()
        .map(|row| {
            let entry = LedgerRow {
                version: row.try_get("version").map_err(stringify)?,
                name: row.try_get("name").map_err(stringify)?,
                checksum: row.try_get("checksum").map_err(stringify)?,
                status: row.try_get("status").map_err(stringify)?,
                applied_at: row.try_get("applied_at").map_err(stringify)?,
            };
            Ok((entry.version.clone(), entry))
        })
        .collect()
}

fn stringify(err: sqlx::Error) -> String {
    format!("unexpected schema_migrations shape: {err}")
}
