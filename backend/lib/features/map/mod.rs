//! The office map: each org member has an (x, y) cell (`map_positions`),
//! defaulting to `(0, 0)` until they've ever moved, plus a `map_active`
//! flag for whether their tab currently has the canvas open. A product
//! feature in its own right, not part of `core::rbac` -- it reaches
//! `core::rbac` only through its public API (`require_member`,
//! `list_members`), same as `features::meetings` does, and owns
//! `map_positions` entirely itself rather than adding columns to
//! `org_users`.
//!
//! [`MOVE_CHANNEL`] carries one live cell change to the audience that
//! actually renders it: online *and* map-active members of the same org --
//! narrower than a roster push (name/picture/roles), and far too frequent
//! to piggyback on one. Everyone's starting position instead comes from
//! [`GET /map/org/{org_id}/positions`](positions_route), fetched once on
//! load.
//!
//! [`ACTIVE_CHANNEL`] is the frontend's own report of the canvas
//! mounting/unmounting, `{"org_id", "active"}`. A clean disconnect clears
//! `map_active` everywhere for that user rather than for one "current org"
//! the way `core::rbac::ACTIVE_CHANNEL` tracks `online` -- this table has no
//! such notion of its own, and a disconnected socket can't be watching any
//! org's canvas anyway, so clearing every row is simpler and just as correct.
//!
//! [`proximity`] is the map's other half: walking into someone starts or
//! joins a meeting with them.
//!
//! [`realtime`] takes live moves off this Lambda altogether when an AppSync
//! Events API is configured: clients publish to it and subscribe to it
//! directly, and `POST .../move` only stores. [`MOVE_CHANNEL`] is then
//! unused -- it is the fallback for a local run, with no AppSync to reach.

use std::collections::{HashMap, HashSet};

use axum::extract::{Extension, Path};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::auth::{AuthError, Claims};
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::{retry_on_conflict, Db, DbError};
use crate::core::rbac::{self, RbacError};
use crate::features::meetings::MeetingError;

pub mod proximity;
pub mod realtime;

/// Cells per side of the (square) office grid -- a position is always
/// bound to `0..OFFICE_SIZE`, see [`set_position`].
pub const OFFICE_SIZE: i32 = 100;

/// One live cell change: `{"org_id", "user_id", "x", "y"}`.
pub const MOVE_CHANNEL: web_messenger::Channel = "map_move";

/// The frontend's own report of the canvas mounting/unmounting:
/// `{"org_id", "active"}`.
pub const ACTIVE_CHANNEL: web_messenger::Channel = "map_active";

fn clamp(value: i32) -> i32 {
    value.clamp(0, OFFICE_SIZE - 1)
}

#[derive(Debug)]
pub enum MapError {
    Database(DbError),
    Rbac(RbacError),
    Meeting(MeetingError),
    Auth(AuthError),
}

impl From<AuthError> for MapError {
    fn from(err: AuthError) -> Self {
        Self::Auth(err)
    }
}

impl From<DbError> for MapError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
    }
}

impl From<RbacError> for MapError {
    fn from(err: RbacError) -> Self {
        Self::Rbac(err)
    }
}

impl From<MeetingError> for MapError {
    fn from(err: MeetingError) -> Self {
        Self::Meeting(err)
    }
}

/// So a plain `sqlx::query...(...).await?` composes inside a function
/// returning `Result<_, MapError>`, same as `RbacError`'s own impl.
impl From<sqlx::Error> for MapError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(DbError::from(err))
    }
}

impl IntoResponse for MapError {
    fn into_response(self) -> Response {
        match self {
            Self::Database(err) => err.into_response(),
            Self::Rbac(err) => err.into_response(),
            Self::Meeting(err) => err.into_response(),
            Self::Auth(err) => err.into_response(),
        }
    }
}

#[derive(sqlx::FromRow)]
struct PositionRow {
    user_id: String,
    x: i32,
    y: i32,
    map_active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct MapPosition {
    pub user_id: String,
    pub x: i32,
    pub y: i32,
    pub map_active: bool,
}

/// Every member's cell in `org_id`, defaulting to `(0, 0, false)` for
/// whoever has no `map_positions` row yet (never moved, never opened the
/// canvas) -- `member_ids` names who counts as a member, since this table
/// carries no membership concept of its own.
async fn list_positions(
    db: &Db,
    org_id: &str,
    member_ids: &[String],
) -> Result<Vec<MapPosition>, MapError> {
    let pool = db.pool().await?;
    let rows: Vec<PositionRow> =
        sqlx::query_as("SELECT user_id, x, y, map_active FROM map_positions WHERE org_id = $1")
            .bind(org_id)
            .fetch_all(pool)
            .await?;
    let mut by_id: HashMap<String, PositionRow> = rows
        .into_iter()
        .map(|row| (row.user_id.clone(), row))
        .collect();
    Ok(member_ids
        .iter()
        .map(|id| match by_id.remove(id) {
            Some(row) => MapPosition {
                user_id: id.clone(),
                x: row.x,
                y: row.y,
                map_active: row.map_active.unwrap_or(false),
            },
            None => MapPosition {
                user_id: id.clone(),
                x: 0,
                y: 0,
                map_active: false,
            },
        })
        .collect())
}

/// Set `user_id`'s cell in `org_id`, clamped to the grid, and return what
/// was actually stored. A plain `UPDATE`, falling back to an `INSERT ...
/// ON CONFLICT DO NOTHING` for whoever has no row yet -- `ON CONFLICT DO
/// NOTHING` is the only upsert form used anywhere else in this codebase
/// (see `core::rbac::add_member`, `assign_role`), so this sticks to it too
/// rather than assume DSQL supports `DO UPDATE`. Losing that fallback's
/// race to a concurrent first move is harmless: the losing write just lands
/// on this tab's next step instead, a few hundred ms later.
async fn set_position(
    db: &Db,
    org_id: &str,
    user_id: &str,
    x: i32,
    y: i32,
) -> Result<(i32, i32), MapError> {
    let (x, y) = (clamp(x), clamp(y));
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        let updated = sqlx::query(
            "UPDATE map_positions SET x = $1, y = $2 WHERE org_id = $3 AND user_id = $4",
        )
        .bind(x)
        .bind(y)
        .bind(org_id)
        .bind(user_id)
        .execute(pool)
        .await?;
        if updated.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO map_positions (org_id, user_id, x, y) VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (org_id, user_id) DO NOTHING",
            )
            .bind(org_id)
            .bind(user_id)
            .bind(x)
            .bind(y)
            .execute(pool)
            .await?;
        }
        Ok::<_, sqlx::Error>(())
    })
    .await?;
    Ok((x, y))
}

/// Same fallback shape as [`set_position`], for `map_active` instead.
async fn set_map_active(
    db: &Db,
    org_id: &str,
    user_id: &str,
    active: bool,
) -> Result<(), MapError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        let updated = sqlx::query(
            "UPDATE map_positions SET map_active = $1 WHERE org_id = $2 AND user_id = $3",
        )
        .bind(active)
        .bind(org_id)
        .bind(user_id)
        .execute(pool)
        .await?;
        if updated.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO map_positions (org_id, user_id, map_active) VALUES ($1, $2, $3) \
                 ON CONFLICT (org_id, user_id) DO NOTHING",
            )
            .bind(org_id)
            .bind(user_id)
            .bind(active)
            .execute(pool)
            .await?;
        }
        Ok::<_, sqlx::Error>(())
    })
    .await?;
    Ok(())
}

async fn clear_all_map_active(db: &Db, user_id: &str) -> Result<(), MapError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE map_positions SET map_active = false WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

/// Members of `org_id` a [`MOVE_CHANNEL`] push should reach: online *and*
/// map-active -- see the module doc. "Online" comes from `core::rbac`'s own
/// public API ([`rbac::online_member_ids`]), not a direct query against
/// `org_users` -- that table is `core::rbac`'s to own. The two lookups are
/// independent, so they run concurrently.
async fn move_audience(db: &Db, org_id: &str) -> Result<Vec<String>, MapError> {
    let active = async {
        let pool = db.pool().await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT user_id FROM map_positions WHERE org_id = $1 AND map_active = true",
        )
        .bind(org_id)
        .fetch_all(pool)
        .await?;
        Ok::<_, MapError>(ids)
    };
    let (online, active_ids) = tokio::try_join!(
        async { Ok::<_, MapError>(rbac::online_member_ids(db, org_id).await?) },
        active,
    )?;
    let online: HashSet<String> = online.into_iter().collect();
    Ok(active_ids
        .into_iter()
        .filter(|id| online.contains(id))
        .collect())
}

#[derive(Deserialize)]
struct MoveBody {
    x: i32,
    y: i32,
    /// Whether to also write this cell to `map_positions`. Only some moves
    /// are stored: the frontend sets this when movement stops, at most once
    /// a second while moving, and on canvas unmount -- so a held key's every
    /// step costs a push, not a write. With AppSync configured (see
    /// [`realtime`]) the frontend sends only these; the pushes go to AppSync
    /// directly.
    #[serde(default)]
    persist: bool,
}

/// `GET /map/org/{org_id}/positions` -- every member's current cell, for
/// initial placement (live updates afterward come from [`MOVE_CHANNEL`]).
async fn positions_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, MapError> {
    rbac::require_member(&db, &org_id, &claims.sub).await?;
    let member_ids: Vec<String> = rbac::list_members(&db, &org_id)
        .await?
        .into_iter()
        .map(|member| member.id)
        .collect();
    let positions = list_positions(&db, &org_id, &member_ids).await?;
    Ok(Json(json!({ "positions": positions })))
}

/// `POST /map/org/{org_id}/move` -- store the caller's (clamped) cell if
/// `persist` is set. Only without AppSync (see [`realtime`]) does this also
/// push it to whoever's tab is actually watching the canvas right now
/// (online *and* `map_active`, not the whole org) -- see [`move_audience`]
/// -- before the write, so the push never waits on it.
async fn move_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<Value>, MapError> {
    let (x, y) = (clamp(body.x), clamp(body.y));

    // The membership check and the audience lookup are both reads, so they
    // share one round trip; nothing is sent or written until the check has
    // passed.
    let audience = async {
        if realtime::endpoint().is_some() {
            return Ok(None);
        }
        Ok::<_, MapError>(Some(move_audience(&db, &org_id).await?))
    };
    let (_, audience) = tokio::try_join!(
        async { Ok::<_, MapError>(rbac::require_member(&db, &org_id, &claims.sub).await?) },
        audience,
    )?;
    if let Some(audience) = audience {
        messenger
            .send(
                MOVE_CHANNEL,
                &audience,
                json!({ "org_id": org_id, "user_id": claims.sub, "x": x, "y": y }),
            )
            .await;
    }
    if body.persist {
        set_position(&db, &org_id, &claims.sub, x, y).await?;
    }
    Ok(Json(json!({ "x": x, "y": y })))
}

/// `/map/*` -- mounted behind [`auth::require_bearer`](crate::core::auth::require_bearer),
/// same as `/rbac/*` and `/meeting/*`.
pub fn routes() -> Router {
    Router::new()
        .route("/org/{org_id}/positions", get(positions_route))
        .route("/org/{org_id}/move", post(move_route))
        .route("/org/{org_id}/proximity", post(proximity::route))
        .route("/org/{org_id}/realtime", get(realtime::route))
}

/// Wire `map_active` to the messenger: a clean disconnect clears it
/// everywhere for that user, and [`ACTIVE_CHANNEL`] carries the frontend's
/// own mount/unmount report. Meant to be called once, at startup.
pub fn register(messenger: &Messenger, db: Db) {
    {
        let db = db.clone();
        messenger.on_message(
            web_messenger::CONNECTION,
            move |_messenger, user_id, message| {
                let db = db.clone();
                async move {
                    if message.as_str() != Some(web_messenger::DISCONNECTED) {
                        return;
                    }
                    if let Err(err) = clear_all_map_active(&db, &user_id).await {
                        tracing::warn!("map: could not clear map_active for {user_id}: {err:?}");
                    }
                }
            },
        );
    }

    messenger.on_message(ACTIVE_CHANNEL, move |_messenger, user_id, message| {
        let db = db.clone();
        async move {
            let org_id = message
                .get("org_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let active = message
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let Some(org_id) = org_id else { return };
            if let Err(err) = set_map_active(&db, &org_id, &user_id, active).await {
                tracing::warn!("map: could not set map_active for {user_id} in {org_id}: {err:?}");
            }
        }
    });
}
