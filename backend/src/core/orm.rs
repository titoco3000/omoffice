//! A tiny builder for describing a table's shape as Rust data, rendering it
//! back to the SQL it stands for, and diffing two shapes into the
//! [`OperationDirection`]s that take one to the other.
//!
//! This is generic low-level machinery — it knows nothing about `users` or
//! `meetings`. `schema.rs` is what declares the actual tables with it, and is
//! the file to read or edit to change the schema; `migrate.rs`'s `generate`
//! is what calls [`diff`] to turn a `schema.rs` edit into migration files.
//!
//! Two shapes of the same idea live here, not one, and that split is
//! deliberate: [`Column`]/[`Table`] hold `&'static str` so `schema.rs` can
//! build them as `const`s (Rust's `const fn` cannot round-trip a type with
//! real drop glue — `Cow<'static, str>` was tried and rejected by the
//! compiler for exactly that reason); [`ColumnState`]/[`TableState`] hold
//! owned `String`s so they can be serialized into
//! `migrations/.snapshot.json` and diffed at runtime. `TableState::from`
//! is the one-way bridge from the first to the second. Nobody hand-writes a
//! `ColumnState`/`TableState` — they're produced from `schema::TABLES` and
//! read back from the snapshot file, the same way nobody hand-edits a
//! lockfile.

// Only `schema.rs`, `migrate.rs`'s `generate`, and this file's own tests use
// this, so plain `cargo build` sees it as unused.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// A `REFERENCES table (column)` on a column.
#[derive(Clone, Copy)]
pub struct ForeignKey {
    pub table: &'static str,
    pub column: &'static str,
}

/// One column. Build with [`Column::new`] and the `.primary_key()` /
/// `.not_null()` / `.default(...)` / `.references(...)` builders.
#[derive(Clone, Copy)]
pub struct Column {
    pub name: &'static str,
    pub sql_type: &'static str,
    pub not_null: bool,
    pub primary_key: bool,
    pub default: Option<&'static str>,
    pub foreign_key: Option<ForeignKey>,
    pub renamed_from: Option<&'static str>,
}

impl Column {
    pub const fn new(name: &'static str, sql_type: &'static str) -> Self {
        Self {
            name,
            sql_type,
            not_null: false,
            primary_key: false,
            default: None,
            foreign_key: None,
            renamed_from: None,
        }
    }

    /// DSQL fixes the primary key at `CREATE TABLE` time — it cannot be added
    /// or changed by a later migration, so this always has to be right from
    /// the first migration that creates the table. [`diff`] refuses a schema
    /// change that flips this on an existing column, rather than emit SQL
    /// DSQL will reject.
    pub const fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self
    }

    pub const fn not_null(mut self) -> Self {
        self.not_null = true;
        self
    }

    pub const fn default(mut self, expr: &'static str) -> Self {
        self.default = Some(expr);
        self
    }

    pub const fn references(mut self, table: &'static str, column: &'static str) -> Self {
        self.foreign_key = Some(ForeignKey { table, column });
        self
    }

    /// Declares this column as `old_name` under a new name, rather than a
    /// brand-new column. [`diff`] can't tell a rename from an unrelated
    /// drop-and-add on its own — two columns disappearing and reappearing
    /// with different names proves nothing about whether they're related —
    /// so it never guesses; this is how you say so instead of it guessing
    /// wrong. `diff` then requires everything *but* the name to be
    /// unchanged (type, `NOT NULL`, default, foreign key, primary key) and
    /// emits a plain `ALTER TABLE ... RENAME COLUMN`, not a drop-and-add —
    /// so the column's data survives, which is the entire point.
    pub const fn renamed_from(mut self, old_name: &'static str) -> Self {
        self.renamed_from = Some(old_name);
        self
    }
}

/// One table: its name and its columns, in the order a migration declares them.
#[derive(Clone, Copy)]
pub struct Table {
    pub name: &'static str,
    pub columns: &'static [Column],
}

/// The owned, JSON-friendly mirror of a [`Column`] — see the module doc
/// comment for why this exists as a second type instead of reusing `Column`
/// directly.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ColumnState {
    pub name: String,
    pub sql_type: String,
    pub not_null: bool,
    pub primary_key: bool,
    pub default: Option<String>,
    /// `(table, column)` — a tuple rather than a second `ForeignKey`-shaped
    /// struct, since there's nothing else this needs to carry.
    pub foreign_key: Option<(String, String)>,
    /// Only ever read off `new` inside [`diff`], never round-tripped through
    /// meaning of its own — see [`Column::renamed_from`]. Once a rename has
    /// been generated, the snapshot already has the new name, so this stops
    /// mattering; it doesn't need to be removed from `schema.rs` afterward,
    /// it just becomes inert.
    pub renamed_from: Option<String>,
}

impl From<&Column> for ColumnState {
    fn from(column: &Column) -> Self {
        Self {
            name: column.name.to_string(),
            sql_type: column.sql_type.to_string(),
            not_null: column.not_null,
            primary_key: column.primary_key,
            default: column.default.map(str::to_string),
            foreign_key: column
                .foreign_key
                .map(|fk| (fk.table.to_string(), fk.column.to_string())),
            renamed_from: column.renamed_from.map(str::to_string),
        }
    }
}

impl ColumnState {
    /// A column's definition inline inside `CREATE TABLE` — DSQL's own
    /// `CREATE TABLE` grammar spells out `REFERENCES` as a column
    /// constraint, same as plain Postgres, so a foreign key belongs right
    /// here. `ALTER TABLE ADD COLUMN` does *not* get this: see
    /// [`OperationDirection::render`] for why a new column's foreign key is
    /// rendered separately there instead of reusing this.
    fn render_for_create_table(&self) -> String {
        let mut out = self.render_definition();
        if let Some((table, column)) = &self.foreign_key {
            out.push_str(&format!(" REFERENCES {table} ({column})"));
        }
        if let Some(default) = &self.default {
            out.push_str(&format!(" DEFAULT {default}"));
        }
        out
    }

    /// A column's definition for `ALTER TABLE ... ADD COLUMN` — deliberately
    /// *without* a foreign key, even if this column has one. DSQL's `ALTER
    /// TABLE` syntax page spells out `ADD COLUMN`'s grammar with no
    /// `column_constraint` clause at all, unlike `CREATE TABLE`'s, so an
    /// inline `REFERENCES` there is unverified; what the same page does
    /// confirm is that a *new* foreign key on `ALTER TABLE` goes through
    /// `ADD CONSTRAINT ... NOT VALID`. [`OperationDirection::render`] adds
    /// that as a second, comma-joined action in the same statement.
    fn render_for_add_column(&self) -> String {
        let mut out = self.render_definition();
        if let Some(default) = &self.default {
            out.push_str(&format!(" DEFAULT {default}"));
        }
        out
    }

    fn render_definition(&self) -> String {
        let mut out = format!("{} {}", self.name, self.sql_type);
        // A primary key is implicitly NOT NULL, so it is not spelled out
        // twice — the migrations do not either.
        if self.primary_key {
            out.push_str(" PRIMARY KEY");
        } else if self.not_null {
            out.push_str(" NOT NULL");
        }
        out
    }
}

/// The owned, JSON-friendly mirror of a [`Table`] — what `migrations/
/// .snapshot.json` actually stores, and what [`diff`] compares.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TableState {
    pub name: String,
    pub columns: Vec<ColumnState>,
}

impl From<&Table> for TableState {
    fn from(table: &Table) -> Self {
        Self {
            name: table.name.to_string(),
            columns: table.columns.iter().map(ColumnState::from).collect(),
        }
    }
}

impl TableState {
    /// The `CREATE TABLE` this table's shape renders to.
    pub fn create_sql(&self) -> String {
        let body = self
            .columns
            .iter()
            .map(ColumnState::render_for_create_table)
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!("CREATE TABLE {} (\n    {body}\n);", self.name)
    }

    /// The `DROP TABLE` that undoes it.
    pub fn drop_sql(&self) -> String {
        format!("DROP TABLE {};", self.name)
    }
}

/// Whether an [`OperationType`] is being added or removed, or (for `Rename`,
/// the one exception) which way a column's name is moving. A migration's
/// `up.sql` is `self.render()`; its `down.sql` is `self.reversed().render()`
/// — the two are never typed out separately, only this one operation is.
#[derive(Clone)]
pub enum OperationDirection {
    Create(OperationType),
    Drop(OperationType),
    /// Not `Create`/`Drop`-shaped like the others — a rename doesn't create
    /// or remove anything, it just moves a name, so undoing it is the same
    /// operation with `from`/`to` swapped rather than an opposite action.
    /// Only [`diff`] constructs one, and only when `Column::renamed_from`
    /// said so explicitly — see there for why it's never inferred.
    Rename {
        table: String,
        from: String,
        to: String,
    },
}

/// One schema change, independent of direction.
/// - `Table` needs its full column list under `Create` (that's the
///   `CREATE TABLE`); under `Drop` only `.name` is read.
/// - `Column` needs its full definition under `Create` (`ADD COLUMN`); under
///   `Drop` only `.name` is read. DSQL also refuses to drop a primary-key
///   column outright, `Create` or not — `render()` panics if you point
///   `Drop` at one rather than hand you SQL that fails at migrate time.
/// - `Condition` names a constraint on an existing column. So far there's
///   only `NotNull`, and it is genuinely one-directional — see [`Condition`].
#[derive(Clone)]
pub enum OperationType {
    Table(TableState),
    Column {
        table: String,
        column: ColumnState,
    },
    Condition {
        table: String,
        column: String,
        condition: Condition,
    },
}

/// A constraint that can be toggled on an existing column without recreating
/// the table.
#[derive(Clone, Copy)]
pub enum Condition {
    /// `Drop` renders `ALTER COLUMN ... DROP NOT NULL`, which DSQL supports.
    /// `Create` (`SET NOT NULL`) does not exist in DSQL's `ALTER TABLE`
    /// grammar at all — see
    /// <https://docs.aws.amazon.com/aurora-dsql/latest/userguide/alter-table-syntax-support.html>,
    /// which lists `DROP NOT NULL` but no `SET NOT NULL`. A column added
    /// nullable is stuck nullable; the only way to require it is a new
    /// column, `NOT NULL` from its own `CREATE`/`ADD COLUMN`. `render()`
    /// panics on `Create(Condition::NotNull)` rather than emit SQL DSQL will
    /// reject, and [`diff`] never constructs one to begin with.
    NotNull,
}

impl OperationDirection {
    /// The opposite change to the same [`OperationType`] — what undoes this
    /// one.
    pub fn reversed(self) -> Self {
        match self {
            Self::Create(op) => Self::Drop(op),
            Self::Drop(op) => Self::Create(op),
            Self::Rename { table, from, to } => Self::Rename {
                table,
                from: to,
                to: from,
            },
        }
    }

    /// The SQL for this operation, in this direction.
    ///
    /// # Panics
    /// On `Create(Condition::NotNull)` (not expressible in DSQL — see
    /// [`Condition::NotNull`]) and on `Drop(Column)` naming a primary key
    /// (DSQL refuses that outright). [`diff`] never produces either; both
    /// are caught here anyway, at render time, as a backstop against a
    /// hand-written `OperationDirection` doing so.
    pub fn render(&self) -> String {
        match self {
            Self::Create(OperationType::Table(table)) => table.create_sql(),
            Self::Drop(OperationType::Table(table)) => format!("DROP TABLE {};", table.name),

            Self::Create(OperationType::Column { table, column }) => {
                let mut stmt = format!(
                    "ALTER TABLE {table} ADD COLUMN {}",
                    column.render_for_add_column()
                );
                // A new foreign key on ALTER TABLE goes through `ADD
                // CONSTRAINT ... NOT VALID`, confirmed by DSQL's own docs --
                // not the inline REFERENCES `render_for_create_table` uses,
                // which is unverified here. `NOT VALID` costs nothing for a
                // brand-new column: it starts NULL on every existing row,
                // and NULL trivially satisfies a foreign key check.
                if let Some((ref_table, ref_column)) = &column.foreign_key {
                    stmt.push_str(&format!(
                        ", ADD FOREIGN KEY ({}) REFERENCES {ref_table} ({ref_column}) NOT VALID",
                        column.name
                    ));
                }
                stmt.push(';');
                stmt
            }
            Self::Drop(OperationType::Column { table, column }) => {
                assert!(
                    !column.primary_key,
                    "DSQL cannot drop the primary key column {}.{} — dropping of primary key \
                     columns is not supported",
                    table, column.name
                );
                format!("ALTER TABLE {table} DROP COLUMN {};", column.name)
            }

            Self::Create(OperationType::Condition {
                table,
                column,
                condition: Condition::NotNull,
            }) => panic!(
                "DSQL has no ALTER COLUMN ... SET NOT NULL (only DROP NOT NULL) — cannot make \
                 {table}.{column} required after the fact; add a new NOT NULL column instead"
            ),
            Self::Drop(OperationType::Condition {
                table,
                column,
                condition: Condition::NotNull,
            }) => format!("ALTER TABLE {table} ALTER COLUMN {column} DROP NOT NULL;"),

            Self::Rename { table, from, to } => {
                format!("ALTER TABLE {table} RENAME COLUMN {from} TO {to};")
            }
        }
    }
}

/// What changed between two schema snapshots, as the `OperationDirection`s
/// (each paired with a suggested migration name) that take `old` to `new`.
/// This is the whole mechanism behind `make db-generate`: diff `schema.rs`
/// (via `TableState::from`) against the last snapshot instead of anyone
/// naming the change themselves.
///
/// Order: new tables first (in `new`'s order — a table an added column
/// references has to exist before that column's own migration runs), then
/// column adds/drops/`NOT NULL` drops per existing table, then dropped
/// tables last.
///
/// Refused outright, rather than guessed at:
/// - a column's type, default or foreign key changing,
/// - a table's or column's primary-key status changing,
/// - an existing column *gaining* `NOT NULL` (DSQL has no `SET NOT NULL`).
///
/// A rename is never inferred, only declared: a column that disappears under
/// one name and reappears under another looks identical, from a name diff
/// alone, to an unrelated drop-and-add, and guessing wrong there is worse
/// than the safe-but-wrong default (a mis-detected rename silently keeps old
/// data alive under the new name instead of visibly failing). So `diff` only
/// ever emits `Rename` when the new column says so itself, via
/// [`Column::renamed_from`] — see there. A disappeared-and-reappeared column
/// with no such hint comes back as a plain `Drop` and `Create`, which loses
/// the column's data; that's still the right call when it's genuinely two
/// unrelated columns, and the hint is how you tell `diff` it isn't.
///
/// A whole *table* renamed this way has no such hint yet and still comes
/// back as `Drop`+`Create` unconditionally — write that one by hand.
pub fn diff(
    old: &[TableState],
    new: &[TableState],
) -> Result<Vec<(String, OperationDirection)>, String> {
    let mut ops = Vec::new();

    for table in new {
        if !old.iter().any(|t| t.name == table.name) {
            ops.push((
                format!("{}_create", table.name),
                OperationDirection::Create(OperationType::Table(table.clone())),
            ));
        }
    }

    for new_table in new {
        let Some(old_table) = old.iter().find(|t| t.name == new_table.name) else {
            continue; // handled as a whole-table create above
        };

        // Columns a rename hint claims, so the drop pass below doesn't also
        // report them as dropped.
        let mut renamed_away: Vec<&str> = Vec::new();

        for column in &new_table.columns {
            match old_table.columns.iter().find(|c| c.name == column.name) {
                Some(existing) => {
                    check_only_supported_change(&new_table.name, existing, column)?;
                    if existing.not_null && !column.not_null {
                        ops.push((
                            format!("{}_drop_not_null_{}", new_table.name, column.name),
                            OperationDirection::Drop(OperationType::Condition {
                                table: new_table.name.clone(),
                                column: column.name.clone(),
                                condition: Condition::NotNull,
                            }),
                        ));
                    }
                }
                None => match &column.renamed_from {
                    Some(old_name) => {
                        let old_column = old_table
                            .columns
                            .iter()
                            .find(|c| &c.name == old_name)
                            .ok_or_else(|| {
                                format!(
                                    "{}.{}: renamed_from(\"{old_name}\") but no such column exists",
                                    new_table.name, column.name
                                )
                            })?;
                        check_pure_rename(&new_table.name, old_column, column)?;
                        ops.push((
                            format!("{}_rename_{old_name}_to_{}", new_table.name, column.name),
                            OperationDirection::Rename {
                                table: new_table.name.clone(),
                                from: old_name.clone(),
                                to: column.name.clone(),
                            },
                        ));
                        renamed_away.push(old_name.as_str());
                    }
                    None => ops.push((
                        format!("{}_add_{}", new_table.name, column.name),
                        OperationDirection::Create(OperationType::Column {
                            table: new_table.name.clone(),
                            column: column.clone(),
                        }),
                    )),
                },
            }
        }

        for column in &old_table.columns {
            if renamed_away.contains(&column.name.as_str()) {
                continue;
            }
            if !new_table.columns.iter().any(|c| c.name == column.name) {
                ops.push((
                    format!("{}_drop_{}", new_table.name, column.name),
                    OperationDirection::Drop(OperationType::Column {
                        table: new_table.name.clone(),
                        column: column.clone(),
                    }),
                ));
            }
        }
    }

    for table in old {
        if !new.iter().any(|t| t.name == table.name) {
            ops.push((
                format!("{}_drop", table.name),
                OperationDirection::Drop(OperationType::Table(table.clone())),
            ));
        }
    }

    Ok(ops)
}

/// Everything about a same-named column that [`diff`] cannot turn into a
/// migration — see its doc comment for why each is refused rather than
/// attempted.
fn check_only_supported_change(
    table: &str,
    old: &ColumnState,
    new: &ColumnState,
) -> Result<(), String> {
    if old.sql_type != new.sql_type {
        return Err(format!(
            "{table}.{}: type changed ({} -> {}) -- write that migration by hand",
            old.name, old.sql_type, new.sql_type
        ));
    }
    if old.default != new.default {
        return Err(format!(
            "{table}.{}: default changed -- write that migration by hand",
            old.name
        ));
    }
    if old.foreign_key != new.foreign_key {
        return Err(format!(
            "{table}.{}: foreign key changed -- write that migration by hand",
            old.name
        ));
    }
    if old.primary_key != new.primary_key {
        return Err(format!(
            "{table}.{}: primary key changed -- DSQL fixes it at CREATE TABLE; this needs a new table",
            old.name
        ));
    }
    if !old.not_null && new.not_null {
        return Err(format!(
            "{table}.{}: cannot add NOT NULL to an existing column -- DSQL has no ALTER COLUMN \
             SET NOT NULL. Add a new NOT NULL column instead.",
            old.name
        ));
    }
    Ok(())
}

/// `RENAME COLUMN` only moves a name — nothing else about the column can
/// ride along in the same migration. Combining a rename with any other
/// change here would need `check_only_supported_change`'s rules applied too
/// (and DSQL's own `RENAME` form takes no other clause to do it in one
/// statement), so it's refused rather than half-supported: do the rename
/// first, then a separate `schema.rs` edit for anything else.
fn check_pure_rename(table: &str, old: &ColumnState, new: &ColumnState) -> Result<(), String> {
    let unchanged = old.sql_type == new.sql_type
        && old.not_null == new.not_null
        && old.primary_key == new.primary_key
        && old.default == new.default
        && old.foreign_key == new.foreign_key;
    if !unchanged {
        return Err(format!(
            "{table}: renaming {} to {} but something else about the column also changed -- \
             generate the rename on its own first, then make the other change separately",
            old.name, new.name
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str, columns: &[(&str, bool)]) -> TableState {
        TableState {
            name: name.to_string(),
            columns: columns
                .iter()
                .map(|(name, not_null)| ColumnState {
                    name: name.to_string(),
                    sql_type: "text".to_string(),
                    not_null: *not_null,
                    primary_key: false,
                    default: None,
                    foreign_key: None,
                    renamed_from: None,
                })
                .collect(),
        }
    }

    #[test]
    fn detects_an_added_column() {
        let old = [table("t", &[("id", false)])];
        let new = [table("t", &[("id", false), ("name", false)])];
        let ops = diff(&old, &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "t_add_name");
        assert!(matches!(
            ops[0].1,
            OperationDirection::Create(OperationType::Column { .. })
        ));
    }

    /// A new column's foreign key must render as a separate `ADD
    /// CONSTRAINT ... NOT VALID` action, not an inline `REFERENCES` on the
    /// `ADD COLUMN` itself — see `ColumnState::render_for_add_column`.
    #[test]
    fn an_added_column_with_a_foreign_key_uses_add_constraint_not_valid() {
        let op = OperationDirection::Create(OperationType::Column {
            table: "meetings".to_string(),
            column: ColumnState {
                name: "host".to_string(),
                sql_type: "text".to_string(),
                not_null: true,
                primary_key: false,
                default: None,
                foreign_key: Some(("users".to_string(), "email".to_string())),
                renamed_from: None,
            },
        });
        assert_eq!(
            op.render(),
            "ALTER TABLE meetings ADD COLUMN host text NOT NULL, \
             ADD FOREIGN KEY (host) REFERENCES users (email) NOT VALID;"
        );
    }

    /// `CREATE TABLE`'s inline `REFERENCES` is unaffected by the above --
    /// DSQL's grammar confirms it there, unlike for `ADD COLUMN`.
    #[test]
    fn a_created_table_still_uses_inline_references() {
        let op = OperationDirection::Create(OperationType::Table(TableState {
            name: "meetings".to_string(),
            columns: vec![ColumnState {
                name: "host".to_string(),
                sql_type: "text".to_string(),
                not_null: true,
                primary_key: false,
                default: None,
                foreign_key: Some(("users".to_string(), "email".to_string())),
                renamed_from: None,
            }],
        }));
        assert_eq!(
            op.render(),
            "CREATE TABLE meetings (\n    host text NOT NULL REFERENCES users (email)\n);"
        );
    }

    #[test]
    fn detects_a_dropped_column() {
        let old = [table("t", &[("id", false), ("name", false)])];
        let new = [table("t", &[("id", false)])];
        let ops = diff(&old, &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "t_drop_name");
    }

    #[test]
    fn detects_a_dropped_not_null() {
        let old = [table("t", &[("name", true)])];
        let new = [table("t", &[("name", false)])];
        let ops = diff(&old, &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "t_drop_not_null_name");
    }

    #[test]
    fn ignores_unrelated_tables() {
        let old = [table("a", &[]), table("b", &[("x", false)])];
        let new = [table("a", &[]), table("b", &[("x", false), ("y", false)])];
        let ops = diff(&old, &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "b_add_y");
    }

    #[test]
    fn detects_a_new_table() {
        let old: [TableState; 0] = [];
        let new = [table("t", &[("id", false)])];
        let ops = diff(&old, &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "t_create");
        assert!(matches!(
            ops[0].1,
            OperationDirection::Create(OperationType::Table(_))
        ));
    }

    #[test]
    fn detects_a_dropped_table() {
        let old = [table("t", &[("id", false)])];
        let new: [TableState; 0] = [];
        let ops = diff(&old, &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "t_drop");
    }

    #[test]
    fn refuses_to_add_not_null_to_an_existing_column() {
        let old = [table("t", &[("name", false)])];
        let new = [table("t", &[("name", true)])];
        assert!(diff(&old, &new).is_err());
    }

    #[test]
    fn refuses_a_type_change() {
        let mut old_col = ColumnState {
            name: "n".to_string(),
            sql_type: "text".to_string(),
            not_null: false,
            primary_key: false,
            default: None,
            foreign_key: None,
            renamed_from: None,
        };
        let old = [TableState {
            name: "t".to_string(),
            columns: vec![old_col.clone()],
        }];
        old_col.sql_type = "int".to_string();
        let new = [TableState {
            name: "t".to_string(),
            columns: vec![old_col],
        }];
        assert!(diff(&old, &new).is_err());
    }

    #[test]
    fn unchanged_schema_diffs_to_nothing() {
        let tables = [table("t", &[("id", false)])];
        assert!(diff(&tables, &tables).unwrap().is_empty());
    }

    #[test]
    fn a_declared_rename_produces_alter_rename_column_not_drop_and_add() {
        let old = [table("t", &[("id", false), ("old_name", false)])];
        let mut new = table("t", &[("id", false)]);
        new.columns.push(ColumnState {
            name: "new_name".to_string(),
            sql_type: "text".to_string(),
            not_null: false,
            primary_key: false,
            default: None,
            foreign_key: None,
            renamed_from: Some("old_name".to_string()),
        });

        let ops = diff(&old, &[new]).unwrap();
        assert_eq!(
            ops.len(),
            1,
            "must be exactly one op, not a drop plus a create"
        );
        assert_eq!(ops[0].0, "t_rename_old_name_to_new_name");
        match &ops[0].1 {
            OperationDirection::Rename { table, from, to } => {
                assert_eq!(table, "t");
                assert_eq!(from, "old_name");
                assert_eq!(to, "new_name");
            }
            _ => panic!("expected a Rename"),
        }
    }

    #[test]
    fn a_rename_naming_a_nonexistent_column_is_an_error() {
        let old = [table("t", &[("id", false)])];
        let mut new = table("t", &[("id", false)]);
        new.columns.push(ColumnState {
            name: "new_name".to_string(),
            sql_type: "text".to_string(),
            not_null: false,
            primary_key: false,
            default: None,
            foreign_key: None,
            renamed_from: Some("does_not_exist".to_string()),
        });
        assert!(diff(&old, &[new]).is_err());
    }

    #[test]
    fn a_rename_bundled_with_another_change_is_refused() {
        let old = [table("t", &[("id", false), ("old_name", false)])];
        let mut new = table("t", &[("id", false)]);
        new.columns.push(ColumnState {
            name: "new_name".to_string(),
            sql_type: "text".to_string(),
            not_null: true, // also changing NOT NULL in the same edit
            primary_key: false,
            default: None,
            foreign_key: None,
            renamed_from: Some("old_name".to_string()),
        });
        assert!(diff(&old, &[new]).is_err());
    }

    #[test]
    fn reversing_a_rename_swaps_the_direction() {
        let op = OperationDirection::Rename {
            table: "t".to_string(),
            from: "a".to_string(),
            to: "b".to_string(),
        };
        assert_eq!(op.render(), "ALTER TABLE t RENAME COLUMN a TO b;");
        assert_eq!(
            op.reversed().render(),
            "ALTER TABLE t RENAME COLUMN b TO a;"
        );
    }
}
