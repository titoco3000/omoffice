//! Describes a table's shape as Rust data: `&'static str` so a table can be
//! declared as a plain `static`. Declaring one with `#[distributed_slice(TABLES)]`
//! is what registers it -- there is no separate list to keep in sync by
//! hand. [`generate`] turns the registered set into migration files;
//! nothing in this module talks to the database directly (see [`super::db`]).

use linkme::distributed_slice;

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
        }
    }

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
}

/// One table: its name and columns. Build with [`Table::new`]; add
/// `.primary_key(&[...])` for a composite key.
#[derive(Clone, Copy)]
pub struct Table {
    pub name: &'static str,
    pub columns: &'static [Column],
    /// Non-empty only for a composite key. Empty means "whichever column, if
    /// any, has its own `.primary_key()` flag set".
    pub primary_key: &'static [&'static str],
}

impl Table {
    pub const fn new(name: &'static str, columns: &'static [Column]) -> Self {
        Self {
            name,
            columns,
            primary_key: &[],
        }
    }

    pub const fn primary_key(mut self, columns: &'static [&'static str]) -> Self {
        self.primary_key = columns;
        self
    }
}

/// Every table that exists, in registration order. A table joins this by
/// declaring itself with `#[distributed_slice(TABLES)]`, not by being added
/// to a list here.
#[distributed_slice]
pub static TABLES: [Table] = [..];

/// Diff the registered [`TABLES`] against the database's current shape and
/// write whatever migration files the difference calls for. The database
/// starts empty for this overhaul, so there is nothing to diff against yet
/// -- see CLAUDE.md for the intended edit -> generate -> apply workflow once
/// there is.
pub fn generate() -> Result<(), String> {
    todo!("diff TABLES against the last applied shape and write .up.sql/.down.sql")
}
