//! The migration runner: loads the `.up.sql`/`.down.sql` pairs
//! [`super::generate`] writes, tracks which have been applied in a ledger
//! table, and applies or reverts them against the DSQL cluster. Nothing
//! here decides what SQL a migration contains -- it only moves the cluster
//! from one recorded state to the next.
//!
//! Applying one is a three-state ledger row, not a single write: DSQL
//! cannot put the DDL and the ledger write in one transaction (each DDL
//! statement is already its own), so a crash mid-migration has to leave a
//! visible `pending`/`reverting` row rather than looking indistinguishable
//! from "never started" or "done".

mod commands;
mod files;
mod ledger;

use crate::core::data::db::Db;

const LEDGER_DDL: &str = "
    CREATE TABLE IF NOT EXISTS schema_migrations (
        version    text        PRIMARY KEY,
        name       text        NOT NULL,
        checksum   text        NOT NULL,
        status     text        NOT NULL,
        started_at timestamptz NOT NULL DEFAULT now(),
        applied_at timestamptz
    )";

const PENDING: &str = "pending";
const APPLIED: &str = "applied";
const REVERTING: &str = "reverting";

/// `<status|up|down|generate>`. Errors are already-worded messages; the
/// caller prints them and exits non-zero.
pub async fn run(args: &[String]) -> Result<(), String> {
    let command = args.first().map(String::as_str).unwrap_or("status");

    // Pure local codegen -- no DATABASE_ENDPOINT, no AWS credentials, no
    // ledger. Handled before anything below touches the database.
    if command == "generate" {
        return super::generate();
    }

    let migrations = files::load(&super::migrations_dir())?;

    let db = Db::from_env().await;
    let pool = db.pool().await.map_err(|_| {
        "no DATABASE_ENDPOINT configured -- set it before running a migrate command besides generate"
            .to_string()
    })?;

    sqlx::query(LEDGER_DDL)
        .execute(pool)
        .await
        .map_err(|err| format!("could not create the schema_migrations ledger: {err}"))?;

    let ledger = ledger::read_ledger(pool).await?;

    match command {
        "status" => {
            commands::status(&migrations, &ledger);
            Ok(())
        }
        "up" => commands::up(pool, &migrations, &ledger).await,
        "down" => commands::down(pool, &migrations, &ledger).await,
        other => Err(format!(
            "unknown migrate command '{other}'; expected status, up, down or generate"
        )),
    }
}
