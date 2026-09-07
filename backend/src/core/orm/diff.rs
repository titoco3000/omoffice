//! Diffing two [`TableState`] snapshots into the SQL that takes one to the
//! other — the whole mechanism behind `make db-generate`.

#[cfg(test)]
use super::schema_types::Column;
use super::state::{ColumnState, TableState};

/// Add/remove an [`OperationType`], or move a column's name. A migration's
/// `up.sql` is `self.render()`; `down.sql` is `self.reversed().render()`.
#[derive(Clone)]
pub enum OperationDirection {
    Create(OperationType),
    Drop(OperationType),
    /// Only [`diff`] builds one, only when [`Column::renamed_from`] said so.
    Rename {
        table: String,
        from: String,
        to: String,
    },
}

/// One schema change. `Table`/`Column` need full definitions under `Create`,
/// only `.name` under `Drop`; `Condition` names a constraint on a column.
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
    /// One-directional: DSQL has `DROP NOT NULL` but no `SET NOT NULL`.
    /// `render()` panics on `Create`; [`diff`] never constructs one.
    NotNull,
}

impl OperationDirection {
    /// The opposite change to the same [`OperationType`] — what undoes this one.
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

    /// The SQL for this operation. Panics on `Create(Condition::NotNull)` or
    /// `Drop(Column)` naming a primary key; [`diff`] never produces either.
    pub fn render(&self) -> String {
        match self {
            Self::Create(OperationType::Table(table)) => table.create_sql(),
            Self::Drop(OperationType::Table(table)) => format!("DROP TABLE {};", table.name),

            Self::Create(OperationType::Column { table, column }) => {
                let mut stmt = format!(
                    "ALTER TABLE {table} ADD COLUMN {}",
                    column.render_for_add_column()
                );
                // `NOT VALID` costs nothing: the column starts NULL on every
                // existing row, which trivially satisfies the check.
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

/// The `OperationDirection`s (each paired with a migration name) that take
/// `old` to `new`. See CLAUDE.md for ordering, refusals, and rename rules.
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

        if old_table.primary_key != new_table.primary_key {
            return Err(format!(
                "{}: primary key changed -- DSQL fixes it at CREATE TABLE; this needs a new table",
                new_table.name
            ));
        }

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
/// migration — see its doc comment for why each is refused rather than attempted.
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

/// `RENAME COLUMN` only moves a name, so a rename bundled with any other
/// change is refused rather than half-supported.
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
#[path = "diff_tests.rs"]
mod tests;
