//! Meetings: the Chime session, the row that remembers it, and the routes.
//!
//! This is the module that knows what a meeting *is*. `chime.rs` only knows how
//! to call Chime, `db.rs` only knows how to reach DSQL, and neither knows about
//! the other — they are the two things this module uses. Domain work belongs
//! here; anything added to either of those two should be usable by a feature
//! that has nothing to do with meetings.
//!
//! * `POST /private/meetings` — create the Chime meeting, record the row, hand
//!   back the code (the Chime `MeetingId` *is* the code).
//! * `POST /private/meetings/{code}/attendees` — look the meeting up by that
//!   code and mint an attendee for the caller. This reads nothing from the
//!   database: the row is history, not state.
//! * `GET /private/meetings` — the caller's past meetings. Chime forgets a
//!   meeting once it empties, so only the table can answer this.
//!
//! The `Meeting`/`Attendee` objects in the responses are keyed exactly as the
//! browser SDK's `MeetingSessionConfiguration` expects; `chime.rs` spells that
//! shape out, because the AWS Rust SDK types are not `Serialize`.

use axum::{
    extract::{Extension, Path},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};

use crate::auth::Claims;
use crate::core::chime::{self, ChimeError, ChimeState};
use crate::core::db::{retry_on_conflict, Db, DbError};
use crate::users;

/// One row of `meetings`, and what `GET /private/meetings` returns.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Meeting {
    pub code: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

impl Meeting {
    /// Record a meeting that was just created in Chime.
    ///
    /// The `users` row is written first because `meetings.created_by`
    /// references it — the caller may hold a token issued before that table
    /// existed.
    pub async fn record(db: &Db, code: &str, created_by: &str) -> Result<(), DbError> {
        users::record(db, created_by).await?;
        let pool = db.pool().await?;
        retry_on_conflict(|| async move {
            sqlx::query(
                "INSERT INTO meetings (code, created_by) VALUES ($1, $2)
                 ON CONFLICT (code) DO NOTHING",
            )
            .bind(code)
            .bind(created_by)
            .execute(pool)
            .await
            .map(|_| ())
        })
        .await
    }

    /// The meetings this email created, newest first.
    pub async fn created_by(db: &Db, email: &str) -> Result<Vec<Self>, DbError> {
        let pool = db.pool().await?;
        let rows = sqlx::query_as::<_, Self>(
            "SELECT code, created_by, created_at
             FROM meetings
             WHERE created_by = $1
             ORDER BY created_at DESC
             LIMIT 100",
        )
        .bind(email)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}

/// `POST /private/meetings` — create a meeting and hand back its code.
pub async fn create(
    Extension(chime): Extension<ChimeState>,
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, MeetingError> {
    let meeting = chime.create_meeting().await?;
    let code = meeting.meeting_id().unwrap_or_default().to_string();

    // Recorded before the code is handed back, and a failure fails the call:
    // a meeting missing from the history is worse than one nobody joined, and
    // Chime drops an empty meeting on its own.
    Meeting::record(&db, &code, &claims.sub).await?;
    tracing::info!("meeting {code} created by {}", claims.sub);

    Ok(Json(json!({
        "code": code,
        "created_by": claims.sub,
        "Meeting": chime::meeting_json(&meeting),
    })))
}

/// `POST /private/meetings/{code}/attendees` — join the meeting named by `code`.
///
/// Returns both halves the browser SDK needs: the meeting it looked up and the
/// attendee it just created for the caller.
pub async fn join(
    Extension(chime): Extension<ChimeState>,
    Extension(claims): Extension<Claims>,
    Path(code): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    let meeting = chime.get_meeting(&code).await?;
    let attendee = chime
        .create_attendee(&code, &chime::external_user_id(&claims.sub))
        .await?;

    tracing::info!("{} joined meeting {code}", claims.sub);

    Ok(Json(json!({
        "code": code,
        "Meeting": chime::meeting_json(&meeting),
        "Attendee": chime::attendee_json(&attendee),
    })))
}

/// `GET /private/meetings` — the meetings this caller has created, newest first.
pub async fn list(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, MeetingError> {
    let meetings = Meeting::created_by(&db, &claims.sub).await?;
    Ok(Json(json!({ "meetings": meetings })))
}

/// What a meeting route can fail with. This module is the only one holding both
/// halves, so it is the only place their errors meet; each half still words its
/// own response.
#[derive(Debug)]
pub enum MeetingError {
    Chime(ChimeError),
    Database(DbError),
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

impl IntoResponse for MeetingError {
    fn into_response(self) -> Response {
        match self {
            Self::Chime(err) => err.into_response(),
            Self::Database(err) => err.into_response(),
        }
    }
}
