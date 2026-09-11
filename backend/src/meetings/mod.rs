//! Meetings: the Chime session, the row that remembers it, and the routes —
//! the only module that knows both Chime and DSQL for a meeting.

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::{AuthError, Claims};
use crate::core::chime::{self, ChimeError, ChimeState};
use crate::core::db::{retry_on_conflict, Db, DbError};
use crate::rbac::{self, RbacError};
use crate::users;
use pool::PoolError;

pub mod invites;
pub mod pool;
pub use invites::*;

/// One row of `meetings`. `org_id` is `None` only for a meeting created
/// before org-scoping existed — every meeting `create` makes now has one.
/// `code` is the DB's `id` column, aliased: which live Chime meeting backs
/// it is never stored here, only in `pool::live`.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Meeting {
    pub code: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub org_id: Option<String>,
}

impl Meeting {
    /// Record a meeting just created under `org_id`, with no live Chime
    /// meeting assigned yet — `pool::live` does that next, once this row
    /// exists for it to point at. The `users` row is written first because
    /// `meetings.created_by` references it.
    pub async fn record(
        db: &Db,
        code: &str,
        created_by: &str,
        org_id: &str,
    ) -> Result<(), DbError> {
        users::record(db, created_by).await?;
        let pool = db.pool().await?;
        retry_on_conflict(|| async move {
            sqlx::query(
                "INSERT INTO meetings (id, created_by, org_id) VALUES ($1, $2, $3)
                 ON CONFLICT (id) DO NOTHING",
            )
            .bind(code)
            .bind(created_by)
            .bind(org_id)
            .execute(pool)
            .await
            .map(|_| ())
        })
        .await
    }

    /// Single-row fetch by code, for `invites`' creator check and `join`'s
    /// existence check.
    pub async fn get(db: &Db, code: &str) -> Result<Option<Self>, DbError> {
        let pool = db.pool().await?;
        let row = sqlx::query_as::<_, Self>(
            "SELECT id AS code, created_by, created_at, org_id FROM meetings WHERE id = $1",
        )
        .bind(code)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// Every meeting in `org_id`, newest first — any member sees and can
    /// rejoin any of them, not just their own.
    pub async fn in_org(db: &Db, org_id: &str) -> Result<Vec<Self>, DbError> {
        let pool = db.pool().await?;
        let rows = sqlx::query_as::<_, Self>(
            "SELECT id AS code, created_by, created_at, org_id
             FROM meetings
             WHERE org_id = $1
             ORDER BY created_at DESC
             LIMIT 100",
        )
        .bind(org_id)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Record that `email` joined `code`, best effort — the only record for
    /// a meeting reached via a bare code or invite link, with no org at all.
    pub async fn record_attendee(db: &Db, code: &str, email: &str) -> Result<(), DbError> {
        users::record(db, email).await?;
        let pool = db.pool().await?;
        retry_on_conflict(|| async move {
            sqlx::query(
                "INSERT INTO meeting_attendees (code, user_email) VALUES ($1, $2)
                 ON CONFLICT (code, user_email) DO NOTHING",
            )
            .bind(code)
            .bind(email)
            .execute(pool)
            .await
            .map(|_| ())
        })
        .await
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateMeetingBody {
    org_id: String,
}

/// `POST /private/meetings` — create a meeting under `org_id` and hand back
/// its own code, never the Chime `MeetingId` `code` used to be. Requires
/// membership. No Chime meeting is claimed here: a room with nobody in it
/// yet (or only its creator) costs nothing until `join` actually needs one
/// — see `pool`.
pub async fn create(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateMeetingBody>,
) -> Result<Json<Value>, MeetingError> {
    rbac::require_member(&db, &claims.sub, &body.org_id).await?;

    let code = Uuid::new_v4().to_string();
    Meeting::record(&db, &code, &claims.sub, &body.org_id).await?;
    tracing::info!(
        "meeting {code} created by {} in org {}",
        claims.sub,
        body.org_id
    );

    Ok(Json(json!({
        "code": code,
        "created_by": claims.sub,
        "org_id": body.org_id,
    })))
}

/// `POST /private/meetings/{code}/attendees` — join Chime for our own
/// `code`, claiming a live Chime meeting (a pooled idle one, or a fresh
/// one) if `code` doesn't already have one. If the pool reassigned its
/// Chime meeting elsewhere (or Chime dropped it), `pool::live` mints a
/// fresh one and claims it back for `code`, rather than dropping the
/// caller into whatever meeting the pool moved it to. The frontend calls
/// this only once someone else is actually on the page — see `presence`.
/// Best-effort records the join; only Chime actually has to succeed here.
pub async fn join(
    Extension(chime): Extension<ChimeState>,
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(code): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    Meeting::get(&db, &code)
        .await?
        .ok_or(MeetingError::NotFound)?;

    let (chime_id, meeting) = pool::live(&db, &chime, &code).await?;

    let attendee = chime
        .create_attendee(&chime_id, &chime::external_user_id(&claims.sub))
        .await?;

    if let Err(err) = Meeting::record_attendee(&db, &code, &claims.sub).await {
        tracing::warn!(
            "could not record {} joining meeting {code}: {err:?}",
            claims.sub
        );
    }
    tracing::info!("{} joined meeting {code}", claims.sub);

    Ok(Json(json!({
        "code": code,
        "Meeting": chime::meeting_json(&meeting),
        "Attendee": chime::attendee_json(&attendee),
    })))
}

#[derive(Debug, Deserialize)]
pub struct ListMeetingsParams {
    org_id: String,
}

/// `GET /private/meetings?org_id=...` — every meeting in that org, newest
/// first; any member sees the whole list, not just their own.
pub async fn list(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<ListMeetingsParams>,
) -> Result<Json<Value>, MeetingError> {
    rbac::require_member(&db, &claims.sub, &params.org_id).await?;
    let meetings = Meeting::in_org(&db, &params.org_id).await?;
    Ok(Json(json!({ "meetings": meetings })))
}

/// What a meeting route can fail with — this module holds both Chime and DB
/// halves, so it's the only place their errors meet.
#[derive(Debug)]
pub enum MeetingError {
    Chime(ChimeError),
    Database(DbError),
    Auth(AuthError),
    /// No `meetings` row for that code — unlike `join`, which only asks
    /// Chime, an invite needs the row to check the creator.
    NotFound,
    /// Caller isn't the meeting's creator.
    Forbidden,
    InviteNotFound,
    /// Already accepted, revoked or declined.
    InviteNotPending,
}

impl From<ChimeError> for MeetingError {
    fn from(err: ChimeError) -> Self {
        Self::Chime(err)
    }
}

impl From<DbError> for MeetingError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
    }
}

impl From<AuthError> for MeetingError {
    fn from(err: AuthError) -> Self {
        Self::Auth(err)
    }
}

/// Collapses `require_member`'s errors to this module's own `Forbidden`,
/// except a database failure, which still reads as `Database`.
impl From<RbacError> for MeetingError {
    fn from(err: RbacError) -> Self {
        match err {
            RbacError::Database(err) => Self::Database(err),
            _ => Self::Forbidden,
        }
    }
}

/// So a plain `sqlx::query...(...).await?` inside [`invites`] composes, same
/// as `RbacError`'s own `From<sqlx::Error>`.
impl From<sqlx::Error> for MeetingError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(DbError::from(err))
    }
}

/// Collapses [`pool::live`]'s errors to this module's own
/// `Chime`/`Database`, same as `RbacError` above.
impl From<PoolError> for MeetingError {
    fn from(err: PoolError) -> Self {
        match err {
            PoolError::Chime(err) => Self::Chime(err),
            PoolError::Database(err) => Self::Database(err),
        }
    }
}

impl IntoResponse for MeetingError {
    fn into_response(self) -> Response {
        match self {
            Self::Chime(err) => err.into_response(),
            Self::Database(err) => err.into_response(),
            Self::Auth(err) => err.into_response(),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "meeting_not_found", "message": "no such meeting" })),
            )
                .into_response(),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "not_meeting_creator",
                    "message": "only the meeting's creator can do this",
                })),
            )
                .into_response(),
            Self::InviteNotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "invite_not_found", "message": "no such invite" })),
            )
                .into_response(),
            Self::InviteNotPending => (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "invite_not_pending",
                    "message": "this invite has already been accepted, revoked or declined",
                })),
            )
                .into_response(),
        }
    }
}
