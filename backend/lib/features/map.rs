//! The office map: each org member has an (x, y) cell, defaulting to
//! `(0, 0)` until they've ever moved, plus a `map_active` flag for whether
//! their tab currently has the canvas open. A product feature in its own
//! right, not part of `core::rbac` -- it reaches `core::rbac` only through
//! its public API (`require_member`, `list_members`), same as
//! `features::meetings` does.
//!
//! The live source of truth is [`MapState`], an in-process
//! `Mutex<HashMap<(org_id, user_id), Position>>`, not `map_positions` --
//! every move/active-toggle only ever touches the map, never the database.
//! `map_positions` is a durability backstop: [`MapState::hydrate`] loads it
//! once at startup, and [`flush_all`]/the disconnect handler below write
//! back to it only on process shutdown or a clean disconnect, so a redeploy
//! or logout doesn't lose a live position. This is safe only because the
//! backend is a single always-on process now -- see `core::rbac`'s module
//! doc for why the same trick would have been wrong under Lambda, where
//! each invocation shares no memory with the next.
//!
//! [`MOVE_CHANNEL`] carries one live cell change to the audience that
//! actually renders it: online *and* map-active members of the same org --
//! narrower than a roster push (name/picture/roles), and far too frequent
//! to piggyback on one. Everyone's starting position instead comes from
//! [`GET /map/org/{org_id}/positions`](positions_route), fetched once on
//! load.
//!
//! [`ACTIVE_CHANNEL`] is the frontend's own report of the canvas
//! mounting/unmounting, `{"org_id", "active"}`. A clean disconnect flushes
//! and clears `map_active` everywhere for that user rather than for one
//! "current org" the way `core::rbac::ACTIVE_CHANNEL` tracks `online` --
//! this table has no such notion of its own, and a disconnected socket
//! can't be watching any org's canvas anyway, so clearing every entry is
//! simpler and just as correct.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::extract::{Extension, Path};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::auth::Claims;
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::{retry_on_conflict, Db, DbError};
use crate::core::rbac::{self, RbacError};

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
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MapPosition {
    pub user_id: String,
    pub x: i32,
    pub y: i32,
    pub map_active: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Position {
    x: i32,
    y: i32,
    map_active: bool,
}

/// The live source of truth for every org member's cell -- see the module
/// doc for why writes stop at this map and only reach `map_positions` on
/// disconnect/shutdown. Keyed by `(org_id, user_id)`, matching that table's
/// primary key. Cheap to `Clone` (an `Arc` underneath), same shape as
/// `ChimeState`.
#[derive(Clone, Default)]
pub struct MapState(Arc<Mutex<HashMap<(String, String), Position>>>);

impl MapState {
    /// One `SELECT` over the whole table, at startup only.
    pub async fn hydrate(db: &Db) -> Self {
        let state = Self::default();
        let Ok(pool) = db.pool().await else {
            return state;
        };
        let rows: Vec<(String, String, i32, i32, Option<bool>)> =
            sqlx::query_as("SELECT org_id, user_id, x, y, map_active FROM map_positions")
                .fetch_all(pool)
                .await
                .unwrap_or_default();
        let mut guard = state.0.lock().unwrap();
        for (org_id, user_id, x, y, map_active) in rows {
            guard.insert(
                (org_id, user_id),
                Position {
                    x,
                    y,
                    map_active: map_active.unwrap_or(false),
                },
            );
        }
        drop(guard);
        state
    }
}

/// Every member's cell in `org_id`, defaulting to `(0, 0, false)` for
/// whoever has no entry yet (never moved, never opened the canvas) --
/// `member_ids` names who counts as a member, since the map carries no
/// membership concept of its own.
fn list_positions(map: &MapState, org_id: &str, member_ids: &[String]) -> Vec<MapPosition> {
    let guard = map.0.lock().unwrap();
    member_ids
        .iter()
        .map(|id| {
            let position = guard
                .get(&(org_id.to_string(), id.clone()))
                .copied()
                .unwrap_or_default();
            MapPosition {
                user_id: id.clone(),
                x: position.x,
                y: position.y,
                map_active: position.map_active,
            }
        })
        .collect()
}

/// Set `user_id`'s cell in `org_id`, clamped to the grid, and return what
/// was actually stored. Map-only -- see the module doc for why this no
/// longer touches the database.
fn set_position(map: &MapState, org_id: &str, user_id: &str, x: i32, y: i32) -> (i32, i32) {
    let (x, y) = (clamp(x), clamp(y));
    let mut guard = map.0.lock().unwrap();
    let entry = guard
        .entry((org_id.to_string(), user_id.to_string()))
        .or_default();
    entry.x = x;
    entry.y = y;
    (x, y)
}

/// Map-only, same shape as [`set_position`].
fn set_map_active(map: &MapState, org_id: &str, user_id: &str, active: bool) {
    let mut guard = map.0.lock().unwrap();
    guard
        .entry((org_id.to_string(), user_id.to_string()))
        .or_default()
        .map_active = active;
}

/// UPSERT `user_id`'s cell in `org_id` into `map_positions` -- the only
/// place this feature still touches the database at runtime (disconnect
/// and process shutdown). `ON CONFLICT DO NOTHING` is the only upsert form
/// used anywhere else in this codebase (see `core::rbac::add_member`,
/// `assign_role`), so this sticks to it too rather than assume DSQL
/// supports `DO UPDATE`; a lost race to a concurrent first flush is
/// harmless since both writers agree on the same in-memory value's source.
async fn flush_position(
    db: &Db,
    org_id: &str,
    user_id: &str,
    position: Position,
) -> Result<(), MapError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        let updated = sqlx::query(
            "UPDATE map_positions SET x = $1, y = $2, map_active = $3 WHERE org_id = $4 AND user_id = $5",
        )
        .bind(position.x)
        .bind(position.y)
        .bind(position.map_active)
        .bind(org_id)
        .bind(user_id)
        .execute(pool)
        .await?;
        if updated.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO map_positions (org_id, user_id, x, y, map_active) VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT (org_id, user_id) DO NOTHING",
            )
            .bind(org_id)
            .bind(user_id)
            .bind(position.x)
            .bind(position.y)
            .bind(position.map_active)
            .execute(pool)
            .await?;
        }
        Ok::<_, sqlx::Error>(())
    })
    .await?;
    Ok(())
}

/// Flush every entry in `map` to `map_positions`. Called on graceful
/// shutdown (see `main.rs`) so a redeploy doesn't lose live positions.
/// Snapshots the map and drops the lock before awaiting each flush, rather
/// than holding it across the loop.
pub async fn flush_all(db: &Db, map: &MapState) {
    let entries: Vec<((String, String), Position)> = {
        let guard = map.0.lock().unwrap();
        guard
            .iter()
            .map(|(key, value)| (key.clone(), *value))
            .collect()
    };
    for ((org_id, user_id), position) in entries {
        if let Err(err) = flush_position(db, &org_id, &user_id, position).await {
            tracing::warn!(
                "map: could not flush position for {user_id} in {org_id} on shutdown: {err:?}"
            );
        }
    }
}

/// Members of `org_id` a [`MOVE_CHANNEL`] push should reach: online *and*
/// map-active -- see the module doc. "Online" comes from
/// `rbac::list_online_member_ids`, not `rbac::list_members`'s full
/// roster-with-roles join -- this runs on every single move, so it only
/// asks for the one column it needs rather than paying for names,
/// pictures, email and roles it would discard. `map_active` comes from the
/// map itself, not `map_positions`, since it's no longer kept fresh in the
/// database in real time.
fn move_audience(map: &MapState, org_id: &str, online: &HashSet<String>) -> Vec<String> {
    let guard = map.0.lock().unwrap();
    guard
        .iter()
        .filter(|((entry_org_id, _), position)| entry_org_id == org_id && position.map_active)
        .filter(|((_, user_id), _)| online.contains(user_id))
        .map(|((_, user_id), _)| user_id.clone())
        .collect()
}

#[derive(Deserialize)]
struct MoveBody {
    x: i32,
    y: i32,
}

/// `GET /map/org/{org_id}/positions` -- every member's current cell, for
/// initial placement (live updates afterward come from [`MOVE_CHANNEL`]).
async fn positions_route(
    Extension(db): Extension<Db>,
    Extension(map): Extension<MapState>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, MapError> {
    rbac::require_member(&db, &org_id, &claims.sub).await?;
    let member_ids: Vec<String> = rbac::list_members(&db, &org_id)
        .await?
        .into_iter()
        .map(|member| member.id)
        .collect();
    let positions = list_positions(&map, &org_id, &member_ids);
    Ok(Json(json!({ "positions": positions })))
}

/// `POST /map/org/{org_id}/move` -- set the caller's cell and push it to
/// whoever's tab is actually watching the canvas right now (online *and*
/// `map_active`, not the whole org) -- see [`move_audience`]. The
/// membership check and the online-id lookup are independent DSQL round
/// trips, so they run concurrently (`tokio::join!`) rather than back to
/// back -- on a route this latency-sensitive (fired on every mouse-move),
/// serializing two round trips would double the wait for no reason.
async fn move_route(
    Extension(db): Extension<Db>,
    Extension(map): Extension<MapState>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<Value>, MapError> {
    let (membership, online) = tokio::join!(
        rbac::require_member(&db, &org_id, &claims.sub),
        rbac::list_online_member_ids(&db, &org_id),
    );
    membership?;
    let online = online?;
    let (x, y) = set_position(&map, &org_id, &claims.sub, body.x, body.y);

    let audience = move_audience(&map, &org_id, &online);
    messenger
        .send(
            MOVE_CHANNEL,
            &audience,
            json!({ "org_id": org_id, "user_id": claims.sub, "x": x, "y": y }),
        )
        .await;
    Ok(Json(json!({ "x": x, "y": y })))
}

/// `/map/*` -- mounted behind [`auth::require_bearer`](crate::core::auth::require_bearer),
/// same as `/rbac/*` and `/meeting/*`.
pub fn routes() -> Router {
    Router::new()
        .route("/org/{org_id}/positions", get(positions_route))
        .route("/org/{org_id}/move", post(move_route))
}

/// Wire `map_active` to the messenger: a clean disconnect flushes every org
/// this user has a live cell in to `map_positions`, then clears
/// `map_active` for those same entries in memory, and [`ACTIVE_CHANNEL`]
/// carries the frontend's own mount/unmount report. Meant to be called
/// once, at startup.
pub fn register(messenger: &Messenger, db: Db, map: MapState) {
    {
        let db = db.clone();
        let map = map.clone();
        messenger.on_message(
            web_messenger::CONNECTION,
            move |_messenger, user_id, message| {
                let db = db.clone();
                let map = map.clone();
                async move {
                    if message.as_str() != Some(web_messenger::DISCONNECTED) {
                        return;
                    }
                    let entries: Vec<(String, Position)> = {
                        let guard = map.0.lock().unwrap();
                        guard
                            .iter()
                            .filter(|((_, entry_user_id), _)| entry_user_id == &user_id)
                            .map(|((org_id, _), position)| (org_id.clone(), *position))
                            .collect()
                    };
                    for (org_id, position) in &entries {
                        if let Err(err) = flush_position(&db, org_id, &user_id, *position).await {
                            tracing::warn!(
                                "map: could not flush position for {user_id} in {org_id}: {err:?}"
                            );
                        }
                    }
                    let mut guard = map.0.lock().unwrap();
                    for (org_id, _) in &entries {
                        if let Some(position) = guard.get_mut(&(org_id.clone(), user_id.clone())) {
                            position.map_active = false;
                        }
                    }
                }
            },
        );
    }

    messenger.on_message(ACTIVE_CHANNEL, move |_messenger, user_id, message| {
        let map = map.clone();
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
            set_map_active(&map, &org_id, &user_id, active);
        }
    });
}
