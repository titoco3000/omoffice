use super::super::schema_types::Table;
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
        primary_key: Vec::new(),
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

/// A new column's foreign key must render as a separate `ADD CONSTRAINT`,
/// not an inline `REFERENCES` on the `ADD COLUMN` itself.
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
        primary_key: Vec::new(),
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
        primary_key: Vec::new(),
    }];
    old_col.sql_type = "int".to_string();
    let new = [TableState {
        name: "t".to_string(),
        columns: vec![old_col],
        primary_key: Vec::new(),
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

#[test]
fn a_composite_primary_key_renders_as_a_trailing_table_constraint() {
    const COLUMNS: &[Column] = &[
        Column::new("org_id", "text").not_null(),
        Column::new("user_email", "text").not_null(),
        Column::new("role", "text").not_null(),
    ];
    let table = Table::new("org_roles", COLUMNS).primary_key(&["org_id", "user_email", "role"]);
    let state = TableState::from(&table);
    assert_eq!(
        state.create_sql(),
        "CREATE TABLE org_roles (\n    \
         org_id text NOT NULL,\n    \
         user_email text NOT NULL,\n    \
         role text NOT NULL,\n    \
         PRIMARY KEY (org_id, user_email, role)\n);"
    );
}

#[test]
fn a_changed_composite_primary_key_is_refused() {
    let mut old = table("t", &[("a", true), ("b", true)]);
    old.primary_key = vec!["a".to_string()];
    let mut new = old.clone();
    new.primary_key = vec!["a".to_string(), "b".to_string()];
    assert!(diff(&[old], &[new]).is_err());
}
