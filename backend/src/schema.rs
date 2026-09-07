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
///
/// `org_id` is nullable rather than required: it was added after `meetings`
/// already had rows with nothing to put there (DSQL's `ADD COLUMN` has no
/// way to backfill a real value per row, only a constant `DEFAULT`, and
/// there is no single org that's the right answer for an existing meeting).
/// `POST /private/meetings` always sets it now -- a `NULL` here only ever
/// means "created before org-scoping existed" and such a meeting won't
/// appear in any org's `GET /private/meetings?org_id=...` list.
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
        Column::new("org_id", "text").references("organizations", "id"),
    ],
);

/// One row per (meeting, person who has actually joined it) --
/// `POST /private/meetings/{code}/attendees` writes one for the caller,
/// best effort, alongside minting their Chime attendee. `GET
/// /private/meetings` reads this to also list meetings the caller was
/// invited into and joined, not only the ones they created -- a meeting's
/// only other row, `meetings.created_by`, never changes once it's created.
/// See `meetings/mod.rs`.
pub const MEETING_ATTENDEES: Table = Table::new(
    "meeting_attendees",
    &[
        Column::new("code", "text")
            .not_null()
            .references("meetings", "code"),
        Column::new("user_email", "text")
            .not_null()
            .references("users", "email"),
        Column::new("joined_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
)
.primary_key(&["code", "user_email"]);

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

/// Live websocket connections to the chat WebSocket API, one row per open
/// browser socket -- a room connection *and* a room-less presence one
/// (`chat::ws_presence` / `chat::lambda_entry` with no `org_id` on the
/// connect query string). Written by the `$connect` route, deleted by
/// `$disconnect` -- both in `chat::connections`. A row surviving its socket
/// (the lambda never got to run `$disconnect`, e.g. a hard client-side
/// crash) is not a correctness problem: sending to it just fails with
/// API Gateway's `GoneException`, and `chat::lambda` deletes the row then,
/// lazily, the next time something tries. See `chat/mod.rs`.
///
/// `org_id` is nullable: NULL means presence-only, no room. This is the one
/// column in the whole schema that started `NOT NULL` and had it dropped
/// later (migration `0008`, hand-written -- `make db-generate` refuses to
/// emit it, since dropping `NOT NULL` from an existing column has no
/// expressible down-migration and `core::orm::OperationDirection::render`
/// panics rather than pretend otherwise; see that migration's own comment).
/// A single connection needs to serve both a chat room *and* an invite's
/// live toast for the same browser tab, so one table beats two: someone
/// already in an org's chat room still needs exactly one socket to reach,
/// not a second one just for presence.
pub const CONNECTIONS: Table = Table::new(
    "connections",
    &[
        Column::new("connection_id", "text").primary_key(),
        Column::new("user_email", "text")
            .not_null()
            .references("users", "email"),
        Column::new("org_id", "text").references("organizations", "id"),
        Column::new("connected_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

/// A pending invitation into an organization. See `rbac.rs`.
pub const ORG_INVITES: Table = Table::new(
    "org_invites",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("org_id", "text")
            .not_null()
            .references("organizations", "id"),
        // No foreign key: the whole point of an invite is that this email
        // may not be a `users` row yet.
        Column::new("email", "text").not_null(),
        // Comma-joined role names, same set `add_member`/`replace_roles`
        // validate against -- empty means "no explicit role", same meaning
        // as an empty `roles` there.
        Column::new("roles", "text").not_null().default("''"),
        Column::new("invited_by", "text")
            .not_null()
            .references("users", "email"),
        // pending | accepted | revoked | declined
        Column::new("status", "text")
            .not_null()
            .default("'pending'"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
        Column::new("accepted_at", "timestamptz"),
    ],
);

/// A pending invitation into a meeting. See `meetings.rs`.
pub const MEETING_INVITES: Table = Table::new(
    "meeting_invites",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("code", "text")
            .not_null()
            .references("meetings", "code"),
        Column::new("email", "text").not_null(),
        Column::new("invited_by", "text")
            .not_null()
            .references("users", "email"),
        Column::new("status", "text")
            .not_null()
            .default("'pending'"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
        Column::new("accepted_at", "timestamptz"),
    ],
);

pub const TABLES: &[Table] = &[
    USERS,
    MEETINGS,
    MEETING_ATTENDEES,
    ORGANIZATIONS,
    ORG_ROLES,
    CONNECTIONS,
    ORG_INVITES,
    MEETING_INVITES,
];

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
