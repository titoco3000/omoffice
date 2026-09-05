//! Every table's *current* shape, in one place: names, types, primary keys,
//! foreign keys — the equivalent of a SQLAlchemy `Table`/`Column` block. This
//! is the only file you edit to change the schema; `Column`/`Table`
//! themselves are generic and live in `core::orm`.
//!
//! It is still *not* how a table gets created: DSQL is only ever reached
//! through `migrations/*.up.sql`, run by `make db-migrate` (see `migrate.rs`
//! for why migrations are hand-written SQL files rather than something a
//! module executes directly — DSQL's DDL/DML transaction rules and the need
//! for a checked-in, reviewable history of schema changes rule that out).
//!
//! The bridge from this file to those files is `make db-generate`
//! (`migrate.rs`'s `generate`): it diffs `TABLES` below (as `TableState`s,
//! see `core::orm`) against `migrations/.snapshot.json` — the shape as of
//! the last time it ran — and writes whatever `.up.sql`/`.down.sql` the
//! difference calls for (`core::orm::diff`). So the workflow is: edit a
//! `Table` below, run `make db-generate`, review what it wrote, `make
//! db-migrate <stage>`. Nothing else needs a matching hand-edit — there is
//! exactly one source of truth.
//!
//! `0001_users`/`0002_meetings` predate this mechanism and were hand-written
//! before there was a snapshot to diff against; `USERS`/`MEETINGS` already
//! describe them correctly (verified by hand when this file was introduced),
//! and every migration since is generated, so nothing here re-checks that
//! correspondence — the test below checks the one invariant that matters
//! going forward: that this file and the snapshot agree.

use crate::core::orm::{Column, Table};

/// People who have asked for an API key. Keyed by the email
/// `/auth/request-key/{email}` already hands out tokens for, so there is no
/// separate id to reconcile. See `users/mod.rs` for the code that reads and
/// writes this table.
pub const USERS: Table = Table::new(
    "users",
    &[
        Column::new("email", "text").primary_key(),
        Column::new("last_meeting", "text").references("meetings", "code"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

/// Meetings created through `POST /private/meetings`. `code` is the Chime
/// `MeetingId`, already a UUID and unique per meeting, so it is the primary
/// key rather than a separate one. See `meetings/mod.rs`.
pub const MEETINGS: Table = Table::new(
    "meetings",
    &[
        Column::new("code", "text").primary_key(),
        Column::new("created_by", "text")
            .not_null()
            .references("users", "email"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

/// Organizations, created through `POST /private/orgs`. `id` is a
/// backend-generated UUID, the same shape as `meetings.code`. See
/// `rbac/mod.rs`.
pub const ORGANIZATIONS: Table = Table::new(
    "organizations",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("name", "text").not_null(),
        Column::new("created_by", "text")
            .not_null()
            .references("users", "email"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

/// The user-org-role table: one row per role a user holds in an org, so a
/// user can hold several roles in the same org at once. See `rbac/mod.rs`.
pub const ORG_ROLES: Table = Table::new(
    "org_roles",
    &[
        Column::new("org_id", "text")
            .not_null()
            .references("organizations", "id"),
        Column::new("user_email", "text")
            .not_null()
            .references("users", "email"),
        Column::new("role", "text").not_null(),
    ],
)
.primary_key(&["org_id", "user_email", "role"]);

/// Live websocket connections to the chat feature's WebSocket API, one row
/// per open browser socket. Written by the `$connect` route, deleted by
/// `$disconnect` -- both in `chat::connections`. A row surviving its socket
/// (the lambda never got to run `$disconnect`, e.g. a hard client-side
/// crash) is not a correctness problem: sending to it just fails with
/// API Gateway's `GoneException`, and `chat::lambda` deletes the row then,
/// lazily, the next time something tries. See `chat/mod.rs`.
pub const CONNECTIONS: Table = Table::new(
    "connections",
    &[
        Column::new("connection_id", "text").primary_key(),
        Column::new("user_email", "text")
            .not_null()
            .references("users", "email"),
        Column::new("org_id", "text")
            .not_null()
            .references("organizations", "id"),
        Column::new("connected_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

pub const TABLES: &[Table] = &[USERS, MEETINGS, ORGANIZATIONS, ORG_ROLES, CONNECTIONS];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::orm::{diff, TableState};

    /// Fails if `TABLES` has changed since the last `make db-generate` —
    /// i.e. if there's an edit here with no migration files to match it yet.
    #[test]
    fn schema_matches_the_last_generated_migration() {
        let path = format!("{}/migrations/.snapshot.json", env!("CARGO_MANIFEST_DIR"));
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!("{path}: {err} (run 'make db-generate' once to create it)")
        });
        let snapshot: Vec<TableState> =
            serde_json::from_str(&raw).unwrap_or_else(|err| panic!("{path}: {err}"));

        let current: Vec<TableState> = TABLES.iter().map(TableState::from).collect();
        let pending = diff(&snapshot, &current).unwrap_or_else(|err| panic!("{err}"));
        assert!(
            pending.is_empty(),
            "schema.rs has changed since the last 'make db-generate': {:?}",
            pending.iter().map(|(name, _)| name).collect::<Vec<_>>()
        );
    }
}
