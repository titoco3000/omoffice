//! The owned, JSON-friendly mirror of [`super::schema_types`]: what the
//! on-disk snapshot stores and what [`super::diff::diff`] compares.

use serde::{Deserialize, Serialize};

use super::schema_types::{Column, Table};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ColumnState {
    pub name: String,
    pub sql_type: String,
    pub not_null: bool,
    pub primary_key: bool,
    pub default: Option<String>,
    /// `(table, column)` -- a tuple rather than a second `ForeignKey`-shaped
    /// struct, since there's nothing else this needs to carry.
    pub foreign_key: Option<(String, String)>,
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
        }
    }
}

impl ColumnState {
    /// A column's definition inline inside `CREATE TABLE`. `ALTER TABLE ADD
    /// COLUMN` does *not* get this -- see [`Self::render_for_add_column`].
    pub(super) fn render_for_create_table(&self) -> String {
        let mut out = self.render_definition();
        if let Some((table, column)) = &self.foreign_key {
            out.push_str(&format!(" REFERENCES {table} ({column})"));
        }
        if let Some(default) = &self.default {
            out.push_str(&format!(" DEFAULT {default}"));
        }
        out
    }

    /// A column's definition for `ADD COLUMN` -- never a foreign key even if
    /// this column has one; see [`super::diff::OperationDirection::render`].
    pub(super) fn render_for_add_column(&self) -> String {
        let mut out = self.render_definition();
        if let Some(default) = &self.default {
            out.push_str(&format!(" DEFAULT {default}"));
        }
        out
    }

    fn render_definition(&self) -> String {
        let mut out = format!("{} {}", self.name, self.sql_type);
        // A primary key is implicitly NOT NULL, so it is not spelled out twice.
        if self.primary_key {
            out.push_str(" PRIMARY KEY");
        } else if self.not_null {
            out.push_str(" NOT NULL");
        }
        out
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TableState {
    pub name: String,
    pub columns: Vec<ColumnState>,
    /// See [`Table::primary_key`]. `#[serde(default)]` so a snapshot written
    /// before this field existed still deserializes, as an empty key.
    #[serde(default)]
    pub primary_key: Vec<String>,
}

impl From<&Table> for TableState {
    fn from(table: &Table) -> Self {
        Self {
            name: table.name.to_string(),
            columns: table.columns.iter().map(ColumnState::from).collect(),
            primary_key: table.primary_key.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl TableState {
    /// The `CREATE TABLE` this table's shape renders to.
    pub fn create_sql(&self) -> String {
        let mut lines: Vec<String> = self
            .columns
            .iter()
            .map(ColumnState::render_for_create_table)
            .collect();
        if !self.primary_key.is_empty() {
            lines.push(format!("PRIMARY KEY ({})", self.primary_key.join(", ")));
        }
        let body = lines.join(",\n    ");
        format!("CREATE TABLE {} (\n    {body}\n);", self.name)
    }

    /// The `DROP TABLE` that undoes it.
    pub fn drop_sql(&self) -> String {
        format!("DROP TABLE {};", self.name)
    }
}
