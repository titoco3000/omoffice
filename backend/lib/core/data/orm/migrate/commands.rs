//! The `status`, `up` and `down` subcommands. `generate` lives in
//! [`crate::core::data::orm`] instead -- it never touches the database.

use std::collections::BTreeMap;

use sqlx::postgres::PgPool;

use super::files::{first_line, statements, Migration};
use super::ledger::LedgerRow;
use super::{APPLIED, PENDING, REVERTING};

pub(super) fn status(migrations: &[Migration], ledger: &BTreeMap<String, LedgerRow>) {
    println!("{:<8}  {:<24}  STATE", "VERSION", "NAME");
    for migration in migrations {
        let state = match ledger.get(&migration.version) {
            None => "not applied".to_string(),
            Some(row) if row.status == APPLIED && row.checksum != migration.checksum => {
                "applied (FILE CHANGED SINCE)".to_string()
            }
            Some(row) if row.status == APPLIED => match row.applied_at {
                Some(at) => format!("applied {}", at.format("%Y-%m-%d %H:%M:%SZ")),
                None => "applied".to_string(),
            },
            Some(row) => format!("INTERRUPTED while {}", row.status),
        };
        println!(
            "{:<8}  {:<24}  {}",
            migration.version, migration.name, state
        );
    }

    // A row with no file left in the repo: someone deleted a migration, or
    // is on a branch that predates it.
    for (version, row) in ledger {
        if !migrations.iter().any(|m| &m.version == version) {
            println!("{:<8}  {:<24}  IN LEDGER, NO FILE", version, row.name);
        }
    }
}

pub(super) async fn up(
    pool: &PgPool,
    migrations: &[Migration],
    ledger: &BTreeMap<String, LedgerRow>,
) -> Result<(), String> {
    check_settled(ledger)?;
    check_checksums(migrations, ledger)?;

    let pending: Vec<&Migration> = migrations
        .iter()
        .filter(|migration| !ledger.contains_key(&migration.version))
        .collect();

    if pending.is_empty() {
        println!("nothing to apply: the database is up to date.");
        return Ok(());
    }

    for migration in pending {
        println!("applying {} {}", migration.version, migration.name);

        sqlx::query(
            "INSERT INTO schema_migrations (version, name, checksum, status)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&migration.version)
        .bind(&migration.name)
        .bind(&migration.checksum)
        .bind(PENDING)
        .execute(pool)
        .await
        .map_err(|err| format!("could not mark {} pending: {err}", migration.version))?;

        execute_all(pool, &migration.up, &migration.version).await?;

        sqlx::query(
            "UPDATE schema_migrations SET status = $2, applied_at = now() WHERE version = $1",
        )
        .bind(&migration.version)
        .bind(APPLIED)
        .execute(pool)
        .await
        .map_err(|err| {
            format!(
                "{} ran, but the ledger could not be updated: {err}. \
                     Re-run after fixing the ledger row by hand.",
                migration.version
            )
        })?;
    }

    println!("done.");
    Ok(())
}

pub(super) async fn down(
    pool: &PgPool,
    migrations: &[Migration],
    ledger: &BTreeMap<String, LedgerRow>,
) -> Result<(), String> {
    check_settled(ledger)?;

    let Some(row) = ledger.values().rfind(|row| row.status == APPLIED) else {
        println!("nothing to revert: no migration is applied.");
        return Ok(());
    };

    let migration = migrations
        .iter()
        .find(|migration| migration.version == row.version)
        .ok_or_else(|| {
            format!(
                "{} is applied but its files are missing; \
                 check out the commit it came from to revert it",
                row.version
            )
        })?;

    println!(
        "reverting {} {} -- this drops whatever it created, and the data in it.",
        migration.version, migration.name
    );

    sqlx::query("UPDATE schema_migrations SET status = $2 WHERE version = $1")
        .bind(&migration.version)
        .bind(REVERTING)
        .execute(pool)
        .await
        .map_err(|err| format!("could not mark {} reverting: {err}", migration.version))?;

    execute_all(pool, &migration.down, &migration.version).await?;

    sqlx::query("DELETE FROM schema_migrations WHERE version = $1")
        .bind(&migration.version)
        .execute(pool)
        .await
        .map_err(|err| {
            format!(
                "{} was reverted, but its ledger row could not be deleted: {err}. \
                 Delete it by hand.",
                migration.version
            )
        })?;

    println!("done.");
    Ok(())
}

/// Run one file's statements, one at a time. Each is its own transaction,
/// which is what DSQL's one-DDL-per-transaction rule requires.
async fn execute_all(pool: &PgPool, sql: &str, version: &str) -> Result<(), String> {
    for statement in statements(sql) {
        let audited = first_line(&statement);
        // The SQL is a checked-in file, never anything a request supplied,
        // which is exactly the audit `AssertSqlSafe` asks for.
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(pool)
            .await
            .map_err(|err| {
                format!("{version} failed and is left marked in the ledger: {err}\n  statement: {audited}")
            })?;
    }
    Ok(())
}

/// Refuse to move while a row is `pending` or `reverting`: that row is the
/// record of a run that died halfway, and only a human knows which half.
fn check_settled(ledger: &BTreeMap<String, LedgerRow>) -> Result<(), String> {
    for row in ledger.values() {
        if row.status != APPLIED {
            return Err(format!(
                "migration {} ({}) was interrupted while {}.\n\
                 The database may or may not carry its change, so nothing further will run.\n\
                 Inspect the schema, then either finish the change and set that row to \
                 '{APPLIED}', or undo it and delete the row.",
                row.version, row.name, row.status
            ));
        }
    }
    Ok(())
}

/// An applied migration whose file has changed since is a silent divergence:
/// the database has the old shape and the repo describes a new one.
fn check_checksums(
    migrations: &[Migration],
    ledger: &BTreeMap<String, LedgerRow>,
) -> Result<(), String> {
    for migration in migrations {
        if let Some(row) = ledger.get(&migration.version) {
            if row.checksum != migration.checksum {
                return Err(format!(
                    "migration {} ({}) has changed since it was applied.\n\
                     Add a new migration instead of editing an applied one.",
                    migration.version, migration.name
                ));
            }
        }
    }
    Ok(())
}
