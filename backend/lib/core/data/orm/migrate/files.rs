//! Loading migration file pairs off disk and splitting one into statements.

use std::path::Path;

pub(super) struct Migration {
    pub(super) version: String,
    pub(super) name: String,
    pub(super) up: String,
    pub(super) down: String,
    pub(super) checksum: String,
}

/// Read every `<version>_<name>.up.sql` and its `.down.sql`, in version order.
pub(super) fn load(dir: &Path) -> Result<Vec<Migration>, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|err| format!("cannot read {}: {err}", dir.display()))?;

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
            .ok_or_else(|| format!("{file}: expected a name like 0001_generated.up.sql"))?;

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
pub(super) fn checksum(sql: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in sql.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub(super) fn first_line(statement: &str) -> String {
    statement
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Split on top-level semicolons, leaving strings/dollar-quotes/comments
/// alone. `has_code` skips a comment-only chunk as a statement of its own --
/// needed because a generated file batches every statement from one run
/// into a single `.up.sql`, and DSQL allows only one DDL statement per
/// transaction.
pub(super) fn statements(sql: &str) -> Vec<String> {
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
    fn a_batched_migration_splits_into_one_statement_per_table() {
        let sql = "CREATE TABLE a (x text);\nCREATE TABLE b (y text);\n";
        assert_eq!(statements(sql).len(), 2);
    }

    #[test]
    fn the_checksum_notices_an_edit() {
        assert_ne!(checksum("CREATE TABLE a ()"), checksum("CREATE TABLE b ()"));
    }
}
