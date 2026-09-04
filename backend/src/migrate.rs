//! The migration runner: `omoffice-api migrate <status|up|down>`.
//!
//! Ordinary migration tools write the ledger row in the same transaction as the
//! schema change, so a crash rolls back both and the two can never disagree.
//! DSQL does not allow DDL and DML in one transaction, so that guarantee is not
//! on offer here — and ordering alone does not replace it. Writing the ledger
//! first and crashing leaves a row claiming a migration that never ran; writing
//! it last and crashing leaves a schema change no row admits to.
//!
//! So the ledger has states, not just rows. Applying is:
//!
//! 1. insert the row as `pending`
//! 2. run the DDL
//! 3. update the row to `applied`
//!
//! and reverting is the mirror image (`applied` -> `reverting` -> row deleted).
//! A crash anywhere leaves a row in an in-between state, and the next run stops
//! with a named error saying which migration was interrupted rather than
//! guessing in either direction.
//!
//! Migrations are ordered pairs of files in `backend/migrations/`:
//! `0001_users.up.sql` and `0001_users.down.sql`. Each file is split into
//! statements and run one at a time, because DSQL allows only one DDL statement
//! per transaction and sending a file as one batch would make it a batch.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::Row;

use crate::core::db::Db;

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
    let migrations = load(&migrations_dir())?;

    let db = Db::from_env().await;
    let pool = db
        .pool()
        .await
        .map_err(|_| "no DATABASE_ENDPOINT: run 'cd ../infra && make output <stage>' and copy the values into backend/.env.<stage>".to_string())?;

    sqlx::query(LEDGER_DDL)
        .execute(pool)
        .await
        .map_err(|err| format!("could not create the schema_migrations ledger: {err}"))?;

    let ledger = read_ledger(pool).await?;

    match command {
        "status" => {
            status(&migrations, &ledger);
            Ok(())
        }
        "up" => up(pool, &migrations, &ledger).await,
        "down" => down(pool, &migrations, &ledger).await,
        other => Err(format!(
            "unknown migrate command '{other}'; expected status, up or down"
        )),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn status(migrations: &[Migration], ledger: &BTreeMap<String, LedgerRow>) {
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

    // A row with no file left in the repo: someone deleted a migration, or is
    // on a branch that predates it.
    for (version, row) in ledger {
        if !migrations.iter().any(|m| &m.version == version) {
            println!("{:<8}  {:<24}  IN LEDGER, NO FILE", version, row.name);
        }
    }
}

async fn up(
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

async fn down(
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
                "{} is applied but its files are missing from {}; \
                 check out the commit it came from to revert it",
                row.version,
                migrations_dir().display()
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

/// Run one file's statements, one at a time. Each is its own transaction, which
/// is what DSQL's one-DDL-per-transaction rule requires.
async fn execute_all(pool: &PgPool, sql: &str, version: &str) -> Result<(), String> {
    for statement in statements(sql) {
        let audited = first_line(&statement);
        // The SQL is a checked-in file, never anything a request supplied,
        // which is exactly the audit AssertSqlSafe asks for.
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(pool)
            .await
            .map_err(|err| {
                format!(
                    "{version} failed and is left marked in the ledger: {err}\n  statement: {audited}"
                )
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
                 Inspect the schema ('make db-shell <stage>'), then either finish the change and set\n\
                 that row to '{APPLIED}', or undo it and delete the row.",
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

// ---------------------------------------------------------------------------
// The ledger
// ---------------------------------------------------------------------------

struct LedgerRow {
    version: String,
    name: String,
    checksum: String,
    status: String,
    applied_at: Option<DateTime<Utc>>,
}

/// Keyed by version, so iteration is in migration order.
async fn read_ledger(pool: &PgPool) -> Result<BTreeMap<String, LedgerRow>, String> {
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

// ---------------------------------------------------------------------------
// The files
// ---------------------------------------------------------------------------

struct Migration {
    version: String,
    name: String,
    up: String,
    down: String,
    checksum: String,
}

fn migrations_dir() -> PathBuf {
    match std::env::var("MIGRATIONS_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(DEFAULT_MIGRATIONS_DIR),
    }
}

/// Read every `<version>_<name>.up.sql` and its `.down.sql`, in version order.
fn load(dir: &Path) -> Result<Vec<Migration>, String> {
    let entries = std::fs::read_dir(dir).map_err(|err| {
        format!(
            "cannot read {}: {err} (run this from backend/)",
            dir.display()
        )
    })?;

    let mut migrations = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|err| format!("cannot read {}: {err}", dir.display()))?
            .path();
        let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(stem) = file.strip_suffix(".up.sql") else {
            continue;
        };
        let (version, name) = stem
            .split_once('_')
            .ok_or_else(|| format!("{file}: expected a name like 0001_something.up.sql"))?;

        let down_path = path.with_file_name(format!("{stem}.down.sql"));
        let down = std::fs::read_to_string(&down_path).map_err(|err| {
            format!(
                "{file} has no revert: {} ({err}). Every migration needs a down.",
                down_path.display()
            )
        })?;
        let up = std::fs::read_to_string(&path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;

        migrations.push(Migration {
            version: version.to_string(),
            name: name.to_string(),
            checksum: checksum(&up),
            up,
            down,
        });
    }

    migrations.sort_by(|a, b| a.version.cmp(&b.version));
    Ok(migrations)
}

/// FNV-1a over the up SQL. This only has to notice an edited migration, not
/// resist anyone, so it beats pulling in a hashing crate.
fn checksum(sql: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in sql.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn first_line(statement: &str) -> String {
    statement
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Split a file into statements on top-level semicolons, leaving the ones
/// inside strings, quoted identifiers, dollar quotes and comments alone.
///
/// `has_code` is what keeps a chunk that is only comments and whitespace — the
/// header above a statement, or the tail of a file — from being sent as a
/// statement of its own.
fn statements(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut has_code = false;

    while i < bytes.len() {
        match bytes[i] {
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            quote @ (b'\'' | b'"') => {
                has_code = true;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == quote {
                        // A doubled quote is an escaped one, not the end.
                        if bytes.get(i + 1) == Some(&quote) {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'$' => {
                has_code = true;
                match dollar_tag(bytes, i) {
                    Some(tag) => {
                        i += tag.len();
                        while i < bytes.len() && !bytes[i..].starts_with(tag) {
                            i += 1;
                        }
                        i = (i + tag.len()).min(bytes.len());
                    }
                    None => i += 1,
                }
            }
            b';' => {
                if has_code {
                    push_statement(&mut out, &sql[start..i]);
                }
                i += 1;
                start = i;
                has_code = false;
            }
            byte => {
                if !byte.is_ascii_whitespace() {
                    has_code = true;
                }
                i += 1;
            }
        }
    }

    if has_code {
        push_statement(&mut out, &sql[start..]);
    }
    out
}

/// `$$` or `$tag$` starting at `i`, if that is what this `$` opens.
fn dollar_tag(bytes: &[u8], i: usize) -> Option<&[u8]> {
    let end = bytes[i + 1..]
        .iter()
        .position(|byte| *byte == b'$')
        .map(|offset| i + 1 + offset)?;
    let inner = &bytes[i + 1..end];
    inner
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .then(|| &bytes[i..=end])
}

fn push_statement(out: &mut Vec<String>, statement: &str) {
    let trimmed = statement.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_top_level_semicolons_only() {
        let sql = "CREATE TABLE a (x text); INSERT INTO a VALUES ('one;two');";
        let split = statements(sql);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0], "CREATE TABLE a (x text)");
        assert_eq!(split[1], "INSERT INTO a VALUES ('one;two')");
    }

    #[test]
    fn leaves_semicolons_in_comments_and_dollar_quotes_alone() {
        let sql = "-- drop this; really\nSELECT $tag$a ; b$tag$;\n/* and; this */";
        let split = statements(sql);
        assert_eq!(split.len(), 1);
        assert!(split[0].contains("$tag$a ; b$tag$"));
    }

    #[test]
    fn a_comment_only_file_has_no_statements() {
        assert!(statements("-- nothing to do here\n").is_empty());
    }

    #[test]
    fn the_shipped_migrations_are_one_statement_each() {
        // DSQL allows one DDL statement per transaction, and each statement
        // here runs in its own; this is a reminder, not a hard requirement.
        for file in ["0001_users.up.sql", "0002_meetings.up.sql"] {
            let sql = std::fs::read_to_string(format!("migrations/{file}")).unwrap();
            assert_eq!(statements(&sql).len(), 1, "{file}");
        }
    }

    #[test]
    fn the_checksum_notices_an_edit() {
        assert_ne!(checksum("CREATE TABLE a ()"), checksum("CREATE TABLE b ()"));
    }
}
