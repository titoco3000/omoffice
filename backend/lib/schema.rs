use linkme::distributed_slice;

use crate::core::data::orm::{self, Column, Table};

// basic RBAC tables
#[distributed_slice(orm::TABLES)]
static USERS: Table = Table::new(
    "users",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("email", "text"),
        Column::new("name", "text"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

#[distributed_slice(orm::TABLES)]
static ORGANIZATIONS: Table = Table::new(
    "organizations",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("name", "text").not_null(),
        Column::new("created_by", "text")
            .not_null()
            .references("users", "id"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

#[distributed_slice(orm::TABLES)]
static ORG_USERS: Table = Table::new(
    "org_users",
    &[
        Column::new("org_id", "text")
            .not_null()
            .references("organizations", "id"),
        Column::new("user_id", "text")
            .not_null()
            .references("users", "id"),
        Column::new("name", "text"), // custom name of the user inside the org
        Column::new("online", "boolean"),
    ],
)
.primary_key(&["org_id", "user_id"]);

#[distributed_slice(orm::TABLES)]
static ORG_ROLES: Table = Table::new(
    "org_roles",
    &[
        Column::new("org_id", "text")
            .not_null()
            .references("organizations", "id"),
        Column::new("user_id", "text")
            .not_null()
            .references("users", "id"),
        Column::new("role", "text").not_null(),
    ],
)
.primary_key(&["org_id", "user_id", "role"]);

#[distributed_slice(orm::TABLES)]
static ORG_INVITES: Table = Table::new(
    "org_invites",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("org_id", "text")
            .not_null()
            .references("organizations", "id"),
        Column::new("email", "text").not_null(),
        // Comma-joined role names; empty means "no explicit role".
        Column::new("roles", "text").not_null().default("''"),
        Column::new("invited_by", "text")
            .not_null()
            .references("users", "id"),
        // pending | accepted | revoked | declined
        Column::new("status", "text")
            .not_null()
            .default("'pending'"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
        Column::new("last_status_change", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

/// meeting tables

#[distributed_slice(orm::TABLES)]
static MEETINGS: Table = Table::new(
    "meetings",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("created_by", "text")
            .not_null()
            .references("users", "id"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
        Column::new("org_id", "text").references("organizations", "id"),
        Column::new("invite_only", "boolean").default("false"),
        Column::new("idle_since", "timestamptz"),
    ],
);

#[distributed_slice(orm::TABLES)]
static MEETING_ROOMS: Table = Table::new(
    "meeting_rooms",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("assigned_to", "text").references("meetings", "id"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

#[distributed_slice(orm::TABLES)]
static MEETING_INVITES: Table = Table::new(
    "meeting_invites",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("meeting_id", "text")
            .not_null()
            .references("meetings", "id"),
        Column::new("invited_user_id", "text")
            .not_null()
            .references("users", "id"),
        Column::new("invited_by", "text")
            .not_null()
            .references("users", "id"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

#[distributed_slice(orm::TABLES)]
static MEETING_ATTENDEES: Table = Table::new(
    "meeting_attendees",
    &[
        Column::new("meeting_id", "text")
            .not_null()
            .references("meetings", "id"),
        Column::new("user_id", "text")
            .not_null()
            .references("users", "id"),
        Column::new("joined_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
)
.primary_key(&["meeting_id", "user_id"]);

// core::communication::web_messenger

#[distributed_slice(orm::TABLES)]
static WS_CONNECTIONS: Table = Table::new(
    "ws_connections",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("user_id", "text")
            .not_null()
            .references("users", "id"),
        Column::new("connected_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `static` above reached `orm::TABLES` on its own -- nothing here
    /// assembles the list by hand. Order isn't guaranteed, so this checks
    /// membership, not sequence.
    #[test]
    fn every_table_registered_itself() {
        let mut names: Vec<&str> = orm::TABLES.iter().map(|table| table.name).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "meeting_attendees",
                "meeting_invites",
                "meeting_rooms",
                "meetings",
                "org_invites",
                "org_roles",
                "org_users",
                "organizations",
                "users",
                "ws_connections",
            ]
        );
    }
}
