//! Every table's *current* shape, in one place — the only file you edit to
//! change the schema. See CLAUDE.md for the edit -> `make db-generate` -> `make db-migrate` workflow.

use crate::core::orm::{Column, Table};

/// People who have asked for an API key. Keyed by the email
/// `/auth/request-key/{email}` issues tokens for. See `users/mod.rs`.
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

/// `code` is the Chime `MeetingId`, already unique, so it's the primary key.
/// `org_id` is nullable only for pre-org-scoping rows — see CLAUDE.md. See `meetings/mod.rs`.
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

/// One row per (meeting, person who joined it), best effort. See
/// `meetings/mod.rs::record_attendee`.
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
/// backend-generated UUID. See `rbac/mod.rs`.
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

/// One row per open browser socket — who is connected, over which
/// transport, and the context they last reported (`org_id`, `meeting`, both
/// nullable). Read only by `core/messenger/`; see `core/messenger/active_users.rs`.
pub const ACTIVE_USERS: Table = Table::new(
    "active_users",
    &[
        Column::new("connection_id", "text").primary_key(),
        Column::new("user_email", "text")
            .not_null()
            .references("users", "email"),
        // "local" (an in-process socket) or "apigw" (a WebSocket API
        // connection) -- which half of `Messenger`'s delivery owns it.
        Column::new("transport", "text").not_null(),
        Column::new("org_id", "text").references("organizations", "id"),
        Column::new("meeting", "text").references("meetings", "code"),
        Column::new("connected_at", "timestamptz")
            .not_null()
            .default("now()"),
        Column::new("last_seen_at", "timestamptz"),
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
        // Comma-joined role names; empty means "no explicit role".
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
    ACTIVE_USERS,
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
