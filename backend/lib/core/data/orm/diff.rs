//! Diffing two [`TableState`] snapshots into the SQL that takes one to the
//! other -- the whole mechanism behind `generate`.

use super::state::{ColumnState, TableState};

/// Add or remove an [`OperationType`]. A migration's `up.sql` is
/// `self.render()`; `down.sql` is `self.reversed().render()`.
#[derive(Clone)]
pub enum OperationDirection {
    Create(OperationType),
    Drop(OperationType),
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
    /// The opposite change to the same [`OperationType`] -- what undoes this one.
    pub fn reversed(self) -> Self {
        match self {
            Self::Create(op) => Self::Drop(op),
            Self::Drop(op) => Self::Create(op),
        }
    }

    /// The SQL for this operation. Panics on `Create(Condition::NotNull)` or
    /// `Drop(Column)` naming a primary key; [`diff`] never produces either.
    pub fn render(&self) -> String {
        match self {
            Self::Create(OperationType::Table(table)) => table.create_sql(),
            Self::Drop(OperationType::Table(table)) => table.drop_sql(),

            Self::Create(OperationType::Column { table, column }) => {
                // Neither `NOT NULL` nor a foreign key can ever be set here
                // -- DSQL's `ADD COLUMN` refuses any constraint at all, so
                // `diff` already refused to produce this op for either;
                // see its own doc.
                assert!(
                    !column.not_null && column.foreign_key.is_none(),
                    "{table}.{}: `diff` should have refused this before it ever got to `render`",
                    column.name
                );
                format!(
                    "ALTER TABLE {table} ADD COLUMN {};",
                    column.render_for_add_column()
                )
            }
            Self::Drop(OperationType::Column { table, column }) => {
                assert!(
                    !column.primary_key,
                    "DSQL cannot drop the primary key column {}.{} -- dropping of primary key \
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
                "DSQL has no ALTER COLUMN ... SET NOT NULL (only DROP NOT NULL) -- cannot make \
                 {table}.{column} required after the fact; add a new NOT NULL column instead"
            ),
            Self::Drop(OperationType::Condition {
                table,
                column,
                condition: Condition::NotNull,
            }) => format!("ALTER TABLE {table} ALTER COLUMN {column} DROP NOT NULL;"),
        }
    }
}

/// The `OperationDirection`s (each paired with a migration name) that take
/// `old` to `new`. Column renames aren't detected -- a name change reads as
/// a drop plus an add -- and a handful of other changes (type, default,
/// foreign key, primary key, adding `NOT NULL` to an existing column, or
/// adding a *new* column that's `NOT NULL` or references another table --
/// DSQL's `ADD COLUMN` refuses any constraint) are refused rather than
/// guessed at; write those by hand.
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
                None => {
                    // DSQL's `ALTER TABLE ADD COLUMN` refuses any constraint
                    // on the new column at all -- not just paired with
                    // another action in the same statement, a bare `NOT
                    // NULL` or foreign key alone fails the same way
                    // ("ALTER TABLE ADD COLUMN with constraint not
                    // supported"). Only CREATE TABLE can set either, so a
                    // new required/referencing column on a table that
                    // already exists needs the table rebuilt, not an
                    // autogenerated ALTER.
                    if column.not_null {
                        return Err(format!(
                            "{}.{}: cannot add a NOT NULL column to an existing table -- DSQL's \
                             ALTER TABLE ADD COLUMN refuses any constraint. Add it nullable, or \
                             drop and recreate the table by hand.",
                            new_table.name, column.name
                        ));
                    }
                    if column.foreign_key.is_some() {
                        return Err(format!(
                            "{}.{}: cannot add a column with a foreign key to an existing table \
                             -- DSQL's ALTER TABLE ADD COLUMN refuses any constraint. Add it \
                             without one, or drop and recreate the table by hand.",
                            new_table.name, column.name
                        ));
                    }
                    ops.push((
                        format!("{}_add_{}", new_table.name, column.name),
                        OperationDirection::Create(OperationType::Column {
                            table: new_table.name.clone(),
                            column: column.clone(),
                        }),
                    ));
                }
            }
        }

        for column in &old_table.columns {
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

    // Reversed: `old` is stored dependencies-first (see `dependency_order`),
    // so walking it backwards drops a dependent before whatever it
    // references -- the order `DROP TABLE` needs.
    for table in old.iter().rev() {
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
/// migration -- see the module doc for why each is refused rather than attempted.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data::orm::schema_types::Column;

    fn table(name: &str, columns: &[ColumnState]) -> TableState {
        TableState {
            name: name.to_string(),
            columns: columns.to_vec(),
            primary_key: Vec::new(),
        }
    }

    fn col(column: Column) -> ColumnState {
        ColumnState::from(&column)
    }

    #[test]
    fn a_brand_new_table_is_a_create() {
        let new = [table(
            "widgets",
            &[col(Column::new("id", "text").primary_key())],
        )];
        let ops = diff(&[], &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "widgets_create");
        assert!(matches!(
            ops[0].1,
            OperationDirection::Create(OperationType::Table(_))
        ));
    }

    #[test]
    fn a_removed_table_is_a_drop() {
        let old = [table("widgets", &[])];
        let ops = diff(&old, &[]).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "widgets_drop");
        assert!(matches!(
            ops[0].1,
            OperationDirection::Drop(OperationType::Table(_))
        ));
    }

    #[test]
    fn an_added_column_is_add_column() {
        let old = [table(
            "widgets",
            &[col(Column::new("id", "text").primary_key())],
        )];
        let new = [table(
            "widgets",
            &[
                col(Column::new("id", "text").primary_key()),
                col(Column::new("name", "text")),
            ],
        )];
        let ops = diff(&old, &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "widgets_add_name");
    }

    #[test]
    fn a_removed_column_is_drop_column() {
        let old = [table(
            "widgets",
            &[
                col(Column::new("id", "text").primary_key()),
                col(Column::new("name", "text")),
            ],
        )];
        let new = [table(
            "widgets",
            &[col(Column::new("id", "text").primary_key())],
        )];
        let ops = diff(&old, &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "widgets_drop_name");
    }

    #[test]
    fn dropping_not_null_is_supported() {
        let old = [table(
            "widgets",
            &[col(Column::new("name", "text").not_null())],
        )];
        let new = [table("widgets", &[col(Column::new("name", "text"))])];
        let ops = diff(&old, &new).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "widgets_drop_not_null_name");
    }

    #[test]
    fn adding_not_null_to_an_existing_column_is_refused() {
        let old = [table("widgets", &[col(Column::new("name", "text"))])];
        let new = [table(
            "widgets",
            &[col(Column::new("name", "text").not_null())],
        )];
        assert!(diff(&old, &new).is_err());
    }

    #[test]
    fn adding_a_new_not_null_column_to_an_existing_table_is_refused() {
        let old = [table(
            "widgets",
            &[col(Column::new("id", "text").primary_key())],
        )];
        let new = [table(
            "widgets",
            &[
                col(Column::new("id", "text").primary_key()),
                col(Column::new("name", "text").not_null()),
            ],
        )];
        assert!(diff(&old, &new).is_err());
    }

    #[test]
    fn adding_a_new_column_with_a_foreign_key_to_an_existing_table_is_refused() {
        let old = [
            table("widgets", &[col(Column::new("id", "text").primary_key())]),
            table("owners", &[col(Column::new("id", "text").primary_key())]),
        ];
        let new = [
            table(
                "widgets",
                &[
                    col(Column::new("id", "text").primary_key()),
                    col(Column::new("owner_id", "text").references("owners", "id")),
                ],
            ),
            table("owners", &[col(Column::new("id", "text").primary_key())]),
        ];
        assert!(diff(&old, &new).is_err());
    }

    #[test]
    fn changing_a_type_is_refused() {
        let old = [table("widgets", &[col(Column::new("count", "int"))])];
        let new = [table("widgets", &[col(Column::new("count", "text"))])];
        assert!(diff(&old, &new).is_err());
    }

    #[test]
    fn changing_a_primary_key_is_refused() {
        let old = [table("widgets", &[])];
        let new = TableState {
            name: "widgets".to_string(),
            columns: Vec::new(),
            primary_key: vec!["id".to_string()],
        };
        assert!(diff(&old, &[new]).is_err());
    }

    #[test]
    fn nothing_changed_is_no_ops() {
        let both = [table(
            "widgets",
            &[col(Column::new("id", "text").primary_key())],
        )];
        assert!(diff(&both, &both).unwrap().is_empty());
    }

    #[test]
    fn reversed_flips_create_and_drop() {
        let create = OperationDirection::Create(OperationType::Table(table("widgets", &[])));
        assert!(matches!(
            create.reversed(),
            OperationDirection::Drop(OperationType::Table(_))
        ));
    }
}
