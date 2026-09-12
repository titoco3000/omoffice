//! The `const`-buildable half: `&'static str` so a table can be declared as
//! a plain `static`. See [`super::state`] for the owned mirror this renders
//! into JSON and diffs against.

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

    /// DSQL fixes the primary key at `CREATE TABLE` time; [`super::diff::diff`]
    /// refuses a change that flips it on an existing column.
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

/// One table: its name and columns, in migration order. Build with
/// [`Table::new`]; add `.primary_key(&[...])` for a composite key.
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

    /// Declares a composite primary key, fixed at `CREATE TABLE` time same
    /// as a single-column one -- only matters for a brand-new table.
    pub const fn primary_key(mut self, columns: &'static [&'static str]) -> Self {
        self.primary_key = columns;
        self
    }
}
