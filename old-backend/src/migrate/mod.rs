//! The migration runner: `omoffice-api migrate <status|up|down|generate>`.
//! See CLAUDE.md for why applying one is a three-state ledger, not one write.

mod commands;
mod files;
mod ledger;

/// `generate`'s record of the schema as of the last run, checked in beside
/// the migrations it produced.
const SNAPSHOT_FILE: &str = ".snapshot.json";

/// Where the ordered `.up.sql` / `.down.sql` pairs live.
const DEFAULT_MIGRATIONS_DIR: &str = "migrations";

/// The ledger. Created on demand; `IF NOT EXISTS` so every run can assert it.
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

/// `omoffice-api migrate <command>`. Errors are already-worded messages; the
/// caller prints them and exits non-zero.
pub async fn run(args: &[String]) -> Result<(), String> {
    let command = args.first().map(String::as_str).unwrap_or("status");

    // Pure local codegen -- no DATABASE_ENDPOINT, no AWS credentials, no
    // ledger. Handled before anything below touches the database.
    if command == "generate" {
        return commands::generate();
    }

    let migrations = files::load(&files::migrations_dir())?;

    let db = crate::core::db::Db::from_env().await;
    let pool = db
        .pool()
        .await
        .map_err(|_| "no DATABASE_ENDPOINT: run 'cd ../infra && make output <stage>' and copy the values into backend/.env.<stage>".to_string())?;

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
