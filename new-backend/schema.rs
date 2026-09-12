use crate::core::data::orm::{Column, Table};

// basic RBAC tables
pub const USERS: Table = Table::build(
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

pub const ORGANIZATIONS: Table = Table::build(
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

pub const ORG_USERS: Table = Table::build(
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

pub const ORG_ROLES: Table = Table::build(
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

pub const ORG_INVITES: Table = Table::build(
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

pub const MEETINGS: Table = Table::build(
    "meetings",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("created_by", "text")
            .not_null()
            .references("users", "email"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
        Column::new("org_id", "text").references("organizations", "id"),
        Column::new("invite_only", "boolean").default("false"),
    ],
);

pub const ROOMS_POOL: Table = Table::build(
    "rooms_pool",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("assigned_to", "text").references("meetings", "id"),
        Column::new("created_at", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

pub const MEETING_INVITES: Table = Table::build(
    "meeting_invites",
    &[
        Column::new("id", "text").primary_key(),
        Column::new("meeting_id", "text")
            .not_null()
            .references("meetings", "id"),
        Column::new("email", "text").not_null(),
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
        Column::new("last_status_change", "timestamptz")
            .not_null()
            .default("now()"),
    ],
);

pub const MEETING_ATTENDEES: Table = Table::build(
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
