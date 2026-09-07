//! Meetings: the Chime session, the row that remembers it, and the routes.
//!
//! This is the module that knows what a meeting *is*. `chime.rs` only knows how
//! to call Chime, `db.rs` only knows how to reach DSQL, and neither knows about
//! the other — they are the two things this module uses. Domain work belongs
//! here; anything added to either of those two should be usable by a feature
//! that has nothing to do with meetings.
//!
//! * `POST /private/meetings` — create the Chime meeting under the caller's
//!   chosen org (they must be a member), record the row, hand back the code
//!   (the Chime `MeetingId` *is* the code).
//! * `POST /private/meetings/{code}/attendees` — look the meeting up by that
//!   code and mint an attendee for the caller. Reads nothing from the
//!   database (the meeting itself is Chime's, not a query); writes a
//!   `meeting_attendees` row, best effort, purely as a record that this
//!   person was there -- listing no longer reads it, see below.
//! * `GET /private/meetings?org_id=...` — every meeting that org has, not
//!   just the caller's own: a meeting is a shared org resource (like the
//!   org's chat room), so any member can see and rejoin any of them. Chime
//!   forgets a meeting once it empties, so only the table can answer this.
//!
//! The `Meeting`/`Attendee` objects in the responses are keyed exactly as the
//! browser SDK's `MeetingSessionConfiguration` expects; `chime.rs` spells that
//! shape out, because the AWS Rust SDK types are not `Serialize`.
//!
//! Meeting invites (`meeting_invites`, owned by this module the same way
//! `meetings` itself is) are the smaller twin of `rbac.rs`'s org invites:
//! same pending -> accepted/revoked/declined shape, same public magic-link
//! accept route, but gated on being the meeting's creator rather than a
//! role, and with no roles of its own to carry.

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::{AuthError, AuthState, Claims};
use crate::core::apigw_management::ApiGatewayManagement;
use crate::core::chime::{self, ChimeError, ChimeState};
use crate::core::db::{retry_on_conflict, Db, DbError};
use crate::messenger::Messenger;
use crate::rbac::{self, RbacError};
use crate::users;

/// One row of `meetings`, and what `GET /private/meetings` returns.
/// `org_id` is `None` only for a meeting created before org-scoping existed
/// -- see `schema::MEETINGS`'s doc comment; every meeting `create` makes now
/// always has one.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Meeting {
    pub code: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub org_id: Option<String>,
}

impl Meeting {
    /// Record a meeting that was just created in Chime, under `org_id`.
    ///
    /// The `users` row is written first because `meetings.created_by`
    /// references it — the caller may hold a token issued before that table
    /// existed.
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
                "INSERT INTO meetings (code, created_by, org_id) VALUES ($1, $2, $3)
                 ON CONFLICT (code) DO NOTHING",
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

    /// Single-row fetch by code -- `create_invite`/`revoke_invite` need to
    /// check who created the meeting; nothing needed that before, only a
    /// caller's own list ([`in_org`](Self::in_org)).
    pub async fn get(db: &Db, code: &str) -> Result<Option<Self>, DbError> {
        let pool = db.pool().await?;
        let row = sqlx::query_as::<_, Self>(
            "SELECT code, created_by, created_at, org_id FROM meetings WHERE code = $1",
        )
        .bind(code)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// Every meeting that belongs to `org_id`, newest first -- any member of
    /// the org can see and rejoin any of them, not just the one who created
    /// it or has already joined: a meeting is a shared org resource, same as
    /// the chat room `chat/mod.rs` gives each org. [`list`] is the route
    /// that calls this, gated on `org_id` membership.
    pub async fn in_org(db: &Db, org_id: &str) -> Result<Vec<Self>, DbError> {
        let pool = db.pool().await?;
        let rows = sqlx::query_as::<_, Self>(
            "SELECT code, created_by, created_at, org_id
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

    /// Record that `email` has actually joined `code` -- written alongside
    /// minting their Chime attendee ([`join`]), best effort: same
    /// "already-signed, already-real" shape as `auth::request_key`'s write
    /// to `users`. Bookkeeping only: [`in_org`](Self::in_org) doesn't read
    /// it (org membership alone is what a meeting's visibility runs on
    /// now), but a meeting reached without an org at all -- someone who
    /// followed a `meeting_invites` link, or a bare code -- has no other
    /// record that they were ever there.
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
/// its code. Requires membership in that org, checked before Chime is ever
/// called: an org isn't optional context here, it's who the meeting belongs
/// to (see `schema::MEETINGS`).
pub async fn create(
    Extension(chime): Extension<ChimeState>,
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateMeetingBody>,
) -> Result<Json<Value>, MeetingError> {
    rbac::require_member(&db, &claims.sub, &body.org_id).await?;

    let meeting = chime.create_meeting().await?;
    let code = meeting.meeting_id().unwrap_or_default().to_string();

    // Recorded before the code is handed back, and a failure fails the call:
    // a meeting missing from the history is worse than one nobody joined, and
    // Chime drops an empty meeting on its own.
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
        "Meeting": chime::meeting_json(&meeting),
    })))
}

/// `POST /private/meetings/{code}/attendees` — join the meeting named by `code`.
///
/// Returns both halves the browser SDK needs: the meeting it looked up and the
/// attendee it just created for the caller. Also records the join
/// ([`Meeting::record_attendee`]), best effort -- Chime is the only thing
/// that actually has to succeed for joining to work.
pub async fn join(
    Extension(chime): Extension<ChimeState>,
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(code): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    let meeting = chime.get_meeting(&code).await?;
    let attendee = chime
        .create_attendee(&code, &chime::external_user_id(&claims.sub))
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
/// first. Requires membership; a meeting is a shared org resource, so this
/// is not filtered down to "meetings I made or joined" the way it used to
/// be -- any member sees (and can rejoin) the whole org's list.
pub async fn list(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<ListMeetingsParams>,
) -> Result<Json<Value>, MeetingError> {
    rbac::require_member(&db, &claims.sub, &params.org_id).await?;
    let meetings = Meeting::in_org(&db, &params.org_id).await?;
    Ok(Json(json!({ "meetings": meetings })))
}

/// What a meeting route can fail with. This module is the only one holding both
/// halves, so it is the only place their errors meet; each half still words its
/// own response.
#[derive(Debug)]
pub enum MeetingError {
    Chime(ChimeError),
    Database(DbError),
    Auth(AuthError),
    /// No `meetings` row for that code -- `create_invite`/`revoke_invite`
    /// need the row itself (to check the creator), unlike `join`, which
    /// only ever asks Chime.
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

/// `create`/`list` both gate on `rbac::require_member` -- collapses its
/// `Forbidden`/`MemberNotFound`/anything-else down to this module's own
/// `Forbidden` (not a member), while still letting a database failure
/// underneath it read as this module's own `Database`, the same way it
/// already reads a `DbError` coming straight from one of this module's own
/// queries.
impl From<RbacError> for MeetingError {
    fn from(err: RbacError) -> Self {
        match err {
            RbacError::Database(err) => Self::Database(err),
            _ => Self::Forbidden,
        }
    }
}

/// So a plain `sqlx::query...(...).await?` inside the invite functions below
/// composes, same as `RbacError`'s own `From<sqlx::Error>`.
impl From<sqlx::Error> for MeetingError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(DbError::from(err))
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

// ---- invites ----

/// One row of `meeting_invites`.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MeetingInvite {
    pub id: String,
    pub code: String,
    pub email: String,
    pub invited_by: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
}

async fn get_invite(db: &Db, invite_id: &str) -> Result<Option<MeetingInvite>, MeetingError> {
    let pool = db.pool().await?;
    let row = sqlx::query_as::<_, MeetingInvite>(
        "SELECT id, code, email, invited_by, status, created_at, accepted_at
         FROM meeting_invites WHERE id = $1",
    )
    .bind(invite_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Create a pending invite for `email` into meeting `code`. Reuses an
/// existing pending row for the same `(code, email)` rather than piling up
/// duplicates, same as `rbac::create_invite`.
pub async fn create_invite(
    db: &Db,
    code: &str,
    email: &str,
    invited_by: &str,
) -> Result<MeetingInvite, MeetingError> {
    let email = email.trim().to_lowercase();
    let pool = db.pool().await?;

    let existing_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM meeting_invites WHERE code = $1 AND email = $2 AND status = 'pending'",
    )
    .bind(code)
    .bind(&email)
    .fetch_optional(pool)
    .await?;

    let id = match existing_id {
        Some(id) => {
            retry_on_conflict(|| {
                let id = id.clone();
                async move {
                    sqlx::query("UPDATE meeting_invites SET created_at = now() WHERE id = $1")
                        .bind(&id)
                        .execute(pool)
                        .await
                        .map(|_| ())
                }
            })
            .await?;
            id
        }
        None => {
            let id = Uuid::new_v4().to_string();
            retry_on_conflict(|| {
                let id = id.clone();
                let email = email.clone();
                async move {
                    sqlx::query(
                        "INSERT INTO meeting_invites (id, code, email, invited_by)
                         VALUES ($1, $2, $3, $4)",
                    )
                    .bind(&id)
                    .bind(code)
                    .bind(&email)
                    .bind(invited_by)
                    .execute(pool)
                    .await
                    .map(|_| ())
                }
            })
            .await?;
            id
        }
    };

    get_invite(db, &id)
        .await?
        .ok_or(MeetingError::InviteNotFound)
}

/// Every meeting invite still pending for `email` -- the meeting half of
/// `GET /private/invites`.
pub async fn pending_invites_for_email(
    db: &Db,
    email: &str,
) -> Result<Vec<MeetingInvite>, MeetingError> {
    let pool = db.pool().await?;
    let rows = sqlx::query_as::<_, MeetingInvite>(
        "SELECT id, code, email, invited_by, status, created_at, accepted_at
         FROM meeting_invites WHERE email = $1 AND status = 'pending' ORDER BY created_at DESC",
    )
    .bind(email.trim().to_lowercase())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Accept a pending invite: mark it `accepted`. Unlike `rbac::accept_invite`
/// there's no membership table to write -- a meeting code is all
/// `POST /private/meetings/{code}/attendees` needs, so accepting here only
/// proves the invite and logs the invitee in; the frontend still calls
/// `attendees` itself with the token this hands back.
pub async fn accept_invite(db: &Db, invite_id: &str) -> Result<MeetingInvite, MeetingError> {
    let invite = get_invite(db, invite_id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    if invite.status != "pending" {
        return Err(MeetingError::InviteNotPending);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let id = invite.id.clone();
        async move {
            sqlx::query(
                "UPDATE meeting_invites SET status = 'accepted', accepted_at = now() WHERE id = $1",
            )
            .bind(&id)
            .execute(pool)
            .await
            .map(|_| ())
        }
    })
    .await?;
    Ok(invite)
}

/// Revoke a pending invite. `code` is checked so the creator of one meeting
/// can't revoke another meeting's invite by id.
pub async fn revoke_invite(db: &Db, code: &str, invite_id: &str) -> Result<(), MeetingError> {
    let invite = get_invite(db, invite_id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    if invite.code != code {
        return Err(MeetingError::InviteNotFound);
    }
    if invite.status != "pending" {
        return Err(MeetingError::InviteNotPending);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let id = invite.id.clone();
        async move {
            sqlx::query("UPDATE meeting_invites SET status = 'revoked' WHERE id = $1")
                .bind(&id)
                .execute(pool)
                .await
                .map(|_| ())
        }
    })
    .await?;
    Ok(())
}

/// Decline a pending invite -- the invited side's "no thanks". `email` must
/// match the invite's, same check `rbac::decline_invite` makes.
pub async fn decline_invite(db: &Db, invite_id: &str, email: &str) -> Result<(), MeetingError> {
    let invite = get_invite(db, invite_id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    if invite.email != email.trim().to_lowercase() {
        return Err(MeetingError::Forbidden);
    }
    if invite.status != "pending" {
        return Err(MeetingError::InviteNotPending);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let id = invite.id.clone();
        async move {
            sqlx::query("UPDATE meeting_invites SET status = 'declined' WHERE id = $1")
                .bind(&id)
                .execute(pool)
                .await
                .map(|_| ())
        }
    })
    .await?;
    Ok(())
}

/// Email the invite (worded differently depending on `users::exists`) and,
/// if that email already has a live session open, push a toast to it right
/// now. Best effort on both, same as `rbac::notify_invite`.
async fn notify_invite(
    db: &Db,
    messenger: &Messenger,
    management: &ApiGatewayManagement,
    invite: &MeetingInvite,
) {
    let existing_user = users::exists(db, &invite.email).await.unwrap_or(false);
    let accept_url = format!(
        "{}/?invite=meeting:{}",
        crate::core::frontend_base_url(),
        invite.id
    );
    let email_body = json!({
        "template": if existing_user { "meeting_invite_existing_user" } else { "meeting_invite_new_user" },
        "code": invite.code,
        "accept_url": accept_url,
    });
    messenger.email().send(&invite.email, &email_body).await;

    let toast = json!({
        "type": "invite",
        "kind": "meeting",
        "invite_id": invite.id,
        "code": invite.code,
        "message": "You've been invited to a meeting",
    });
    messenger
        .push_toast(db, management, &invite.email, &toast)
        .await;
}

// ---- invite routes ----

#[derive(Debug, Deserialize)]
pub struct CreateMeetingInviteBody {
    email: String,
}

/// `POST /private/meetings/{code}/invites` — invite someone to a meeting.
/// Only the meeting's creator may.
pub async fn create_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Extension(messenger): Extension<Messenger>,
    Extension(management): Extension<ApiGatewayManagement>,
    Path(code): Path<String>,
    Json(body): Json<CreateMeetingInviteBody>,
) -> Result<Json<Value>, MeetingError> {
    let meeting = Meeting::get(&db, &code)
        .await?
        .ok_or(MeetingError::NotFound)?;
    if meeting.created_by != claims.sub {
        return Err(MeetingError::Forbidden);
    }
    let invite = create_invite(&db, &code, &body.email, &claims.sub).await?;
    notify_invite(&db, &messenger, &management, &invite).await;
    Ok(Json(json!({ "invite": invite })))
}

/// `DELETE /private/meetings/{code}/invites/{invite_id}` — revoke. Only the
/// meeting's creator may.
pub async fn revoke_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path((code, invite_id)): Path<(String, String)>,
) -> Result<Json<Value>, MeetingError> {
    let meeting = Meeting::get(&db, &code)
        .await?
        .ok_or(MeetingError::NotFound)?;
    if meeting.created_by != claims.sub {
        return Err(MeetingError::Forbidden);
    }
    revoke_invite(&db, &code, &invite_id).await?;
    Ok(Json(json!({ "revoked": invite_id })))
}

/// `POST /invites/meetings/{invite_id}/accept` — the magic link: **public**,
/// same trust model as `rbac::accept_invite_route`. Accepts and signs the
/// invited email a token in one call; the frontend still calls
/// `POST /private/meetings/{code}/attendees` with it to actually join.
pub async fn accept_invite_route(
    State(auth): State<AuthState>,
    Extension(db): Extension<Db>,
    Path(invite_id): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    let invite = accept_invite(&db, &invite_id).await?;
    let (token, expires_at) = auth.issue(&invite.email).await?;
    Ok(Json(json!({
        "token": token,
        "token_type": "Bearer",
        "email": invite.email,
        "expires_at": expires_at,
        "code": invite.code,
    })))
}

/// `POST /private/invites/meetings/{invite_id}/decline` — the invited
/// side's "no thanks", for someone already logged in.
pub async fn decline_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(invite_id): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    decline_invite(&db, &invite_id, &claims.sub).await?;
    Ok(Json(json!({ "declined": invite_id })))
}
