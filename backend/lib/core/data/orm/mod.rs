//! Describes a table's shape as Rust data ([`schema_types`]), an owned
//! mirror of that shape for diffing and JSON ([`state`]), and the diff
//! itself ([`diff`]). [`generate`] ties them together: it reads the last
//! snapshot off disk, diffs it against every table currently registered in
//! [`TABLES`], and writes whatever `.up.sql`/`.down.sql` pair the
//! difference calls for. [`migrate`] applies what `generate` writes against
//! the DSQL cluster -- this module only talks to the database through it;
//! `generate` itself is pure local codegen (see [`super::db`] for the
//! connection either eventually goes through).

pub mod diff;
pub mod migrate;
pub mod schema_types;
pub mod state;

pub use diff::*;
pub use schema_types::*;
pub use state::*;

use std::path::{Path, PathBuf};

use linkme::distributed_slice;

/// Every table that exists, in registration order. A table joins this by
/// declaring itself with `#[distributed_slice(TABLES)]` -- there is no
/// separate list to keep in sync by hand.
#[distributed_slice]
pub static TABLES: [Table] = [..];

/// `generate`'s record of the schema as of the last run, checked in beside
/// the migrations it produced.
const SNAPSHOT_FILE: &str = ".snapshot.json";

/// Where the ordered `.up.sql`/`.down.sql` pairs live: `MIGRATIONS_DIR`, or
/// `migrations/` next to this crate's `Cargo.toml`. The latter default is
/// independent of the working directory a command happens to run from.
pub(crate) fn migrations_dir() -> PathBuf {
    match std::env::var("MIGRATIONS_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations"),
    }
}

/// Diff every table in [`TABLES`] against the last snapshot and write one
/// `.up.sql`/`.down.sql` pair covering every change the difference calls
/// for -- one run, one migration, however many tables and columns it
/// touches. No snapshot yet means no tables yet either -- diffed against
/// nothing, the first run's tables come out as the initial `CREATE TABLE`s,
/// not a baseline skipped in silence.
pub fn generate() -> Result<(), String> {
    let dir = migrations_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
    let snapshot_path = dir.join(SNAPSHOT_FILE);

    // Registration order isn't guaranteed across compilation units, so this
    // orders itself: dependencies before dependents (a freshly generated
    // `CREATE TABLE` must never reference a table that doesn't exist yet),
    // alphabetical as the tie-break for everything else.
    let new: Vec<TableState> = dependency_order(TABLES.iter().map(TableState::from).collect())?;

    let old: Vec<TableState> = match std::fs::read_to_string(&snapshot_path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|err| format!("cannot parse {}: {err}", snapshot_path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(format!("cannot read {}: {err}", snapshot_path.display())),
    };

    let pending = diff::diff(&old, &new)?;
    if pending.is_empty() {
        println!("nothing to generate: the registered tables match the last generated migration.");
        return Ok(());
    }

    let version = next_version(&dir)?;
    let up_path = dir.join(format!("{version:04}_generated.up.sql"));
    let down_path = dir.join(format!("{version:04}_generated.down.sql"));
    if up_path.exists() || down_path.exists() {
        return Err(format!(
            "{} already exists -- delete it (and its pair) or run generate again",
            up_path.display()
        ));
    }

    // Up runs in dependency order (creates before whatever references them);
    // down undoes that same batch back to front, same as reverting several
    // migrations one at a time would.
    let up_sql = pending
        .iter()
        .map(|(_, op)| op.render())
        .collect::<Vec<_>>()
        .join("\n");
    let down_sql = pending
        .into_iter()
        .rev()
        .map(|(_, op)| op.reversed().render())
        .collect::<Vec<_>>()
        .join("\n");

    std::fs::write(&up_path, generated_sql(&up_sql))
        .map_err(|err| format!("cannot write {}: {err}", up_path.display()))?;
    std::fs::write(&down_path, generated_sql(&down_sql))
        .map_err(|err| format!("cannot write {}: {err}", down_path.display()))?;
    println!("wrote {} and {}", up_path.display(), down_path.display());

    write_snapshot(&snapshot_path, &new)?;
    println!("done. Review the new file(s), then 'migrate up' to apply them.");
    Ok(())
}

fn write_snapshot(path: &Path, tables: &[TableState]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(tables)
        .map_err(|err| format!("cannot serialize the registered tables: {err}"))?;
    std::fs::write(path, json).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

/// The next version number after whatever's already on disk, so a generated
/// migration continues `0001`, `0002`, ... rather than starting over.
fn next_version(dir: &Path) -> Result<u32, String> {
    let mut max = 0u32;
    for entry in
        std::fs::read_dir(dir).map_err(|err| format!("cannot read {}: {err}", dir.display()))?
    {
        let path = entry
            .map_err(|err| format!("cannot read {}: {err}", dir.display()))?
            .path();
        let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(stem) = file.strip_suffix(".up.sql") else {
            continue;
        };
        if let Some((version, _)) = stem.split_once('_') {
            if let Ok(n) = version.parse::<u32>() {
                max = max.max(n);
            }
        }
    }
    Ok(max + 1)
}

/// The file content for one direction of a whole batch: a header saying
/// where it came from, then every statement, one per line.
fn generated_sql(body: &str) -> String {
    format!(
        "-- generated by `core::data::orm::generate` from a schema diff.\n\
         -- change the table and run generate again, not this file.\n\n{body}\n"
    )
}

/// Every table, dependencies (whatever a foreign key points at) before
/// dependents, alphabetical as the tie-break among tables with no ordering
/// constraint between them -- so the result is deterministic despite
/// [`TABLES`]' own registration order not being guaranteed, and safe to
/// generate `CREATE TABLE`s straight down in this order.
fn dependency_order(mut tables: Vec<TableState>) -> Result<Vec<TableState>, String> {
    tables.sort_by(|a, b| a.name.cmp(&b.name));

    let mut sorted: Vec<TableState> = Vec::with_capacity(tables.len());
    let mut remaining = tables;

    while !remaining.is_empty() {
        let ready = remaining.iter().position(|table| {
            table
                .columns
                .iter()
                .all(|column| match &column.foreign_key {
                    // A self-reference needs no earlier table; anything else
                    // must already be placed.
                    Some((ref_table, _)) => {
                        ref_table == &table.name || sorted.iter().any(|t| &t.name == ref_table)
                    }
                    None => true,
                })
        });
        let Some(index) = ready else {
            let names: Vec<&str> = remaining.iter().map(|t| t.name.as_str()).collect();
            return Err(format!(
                "circular (or otherwise unresolvable) foreign key dependency among: {}",
                names.join(", ")
            ));
        };
        sorted.push(remaining.remove(index));
    }
    Ok(sorted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schema_types::Column;

    fn table(name: &str, columns: &[Column]) -> TableState {
        TableState {
            name: name.to_string(),
            columns: columns.iter().map(ColumnState::from).collect(),
            primary_key: Vec::new(),
        }
    }

    #[test]
    fn a_table_comes_after_whatever_it_references() {
        let children = table(
            "children",
            &[Column::new("parent_id", "text").references("parents", "id")],
        );
        let parents = table("parents", &[Column::new("id", "text").primary_key()]);

        // Alphabetically "children" sorts first; dependency order must
        // still place "parents" ahead of it.
        let ordered = dependency_order(vec![children, parents]).unwrap();
        assert_eq!(ordered[0].name, "parents");
        assert_eq!(ordered[1].name, "children");
    }

    #[test]
    fn unrelated_tables_fall_back_to_alphabetical_order() {
        let b = table("b", &[]);
        let a = table("a", &[]);
        let ordered = dependency_order(vec![b, a]).unwrap();
        assert_eq!(ordered[0].name, "a");
        assert_eq!(ordered[1].name, "b");
    }

    #[test]
    fn a_self_reference_is_not_a_cycle() {
        let node = table(
            "nodes",
            &[Column::new("parent_id", "text").references("nodes", "id")],
        );
        assert!(dependency_order(vec![node]).is_ok());
    }

    #[test]
    fn an_unresolvable_cycle_is_refused() {
        let a = table("a", &[Column::new("b_id", "text").references("b", "id")]);
        let b = table("b", &[Column::new("a_id", "text").references("a", "id")]);
        assert!(dependency_order(vec![a, b]).is_err());
    }
}
