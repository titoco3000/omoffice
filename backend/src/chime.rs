//! Amazon Chime SDK meetings.
//!
//! Two routes, both under `/private`, so a caller needs a bearer token:
//!
//! * `POST /private/meetings` — create a meeting. The Chime `MeetingId` *is*
//!   the meeting code; the only thing kept on our side is a row in `meetings`
//!   recording who created it, which is what `GET /private/meetings` reads.
//!   Joining still needs nothing but the code — the row is history, not state.
//! * `POST /private/meetings/{code}/attendees` — look the meeting up by that
//!   code and mint an attendee for the caller.
//!
//! Both responses carry `Meeting` / `Attendee` objects keyed exactly as the
//! browser SDK's `MeetingSessionConfiguration` expects them, because that is
//! what the frontend feeds it. The AWS Rust SDK types are not `Serialize`, so
//! [`meeting_json`] and [`attendee_json`] spell the shape out.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use aws_sdk_chimesdkmeetings::types::{Attendee, Meeting};
use aws_sdk_chimesdkmeetings::Client;
use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::auth::Claims;
use crate::db::{Db, DbError};

/// Region the media session runs in when nothing else says otherwise.
const DEFAULT_MEDIA_REGION: &str = "us-east-1";

/// Chime client plus the region meetings are created in. Cheap to clone; the
/// client is `None` in tests, where the routes answer 503 instead of calling AWS.
#[derive(Clone)]
pub struct ChimeState {
    client: Option<Client>,
    media_region: String,
}

impl ChimeState {
    /// No AWS behind it: every route reports `chime_unavailable`. Tests only —
    /// a real run always has a client, even if the credentials behind it fail.
    #[cfg(test)]
    pub fn disabled() -> Self {
        Self {
            client: None,
            media_region: DEFAULT_MEDIA_REGION.to_string(),
        }
    }

    /// Build the client from the ambient AWS config — the lambda's execution
    /// role in AWS, your `AWS_PROFILE` under `make run`.
    ///
    /// The media region is `CHIME_MEDIA_REGION`, else the region the SDK
    /// resolved, else `us-east-1`. It is where the audio/video is mixed, and
    /// does not have to be the region the API runs in.
    pub async fn from_env() -> Self {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let media_region = std::env::var("CHIME_MEDIA_REGION")
            .ok()
            .filter(|region| !region.is_empty())
            .or_else(|| config.region().map(|region| region.to_string()))
            .unwrap_or_else(|| DEFAULT_MEDIA_REGION.to_string());

        tracing::info!("chime media region: {media_region}");
        Self {
            client: Some(Client::new(&config)),
            media_region,
        }
    }

    fn client(&self) -> Result<&Client, ChimeError> {
        self.client.as_ref().ok_or(ChimeError::Unavailable)
    }
}

/// What the routes can fail with.
#[derive(Debug)]
pub enum ChimeError {
    /// No Chime client was built — tests, or a local run with no AWS config.
    Unavailable,
    /// The code does not name a live meeting. Chime drops empty meetings, so
    /// this is also what a stale code looks like.
    MeetingNotFound,
    /// Anything the Chime API itself rejected.
    Upstream(String),
    /// The meeting was made, but recording it failed.
    Database(DbError),
}

impl From<DbError> for ChimeError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
    }
}

impl IntoResponse for ChimeError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            // The database layer already words its own failures.
            Self::Database(err) => return err.into_response(),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "chime_unavailable",
                "this API instance has no Chime client configured".to_string(),
            ),
            Self::MeetingNotFound => (
                StatusCode::NOT_FOUND,
                "meeting_not_found",
                "no live meeting has that code; it may have ended".to_string(),
            ),
            Self::Upstream(why) => {
                tracing::error!("chime call failed: {why}");
                (
                    StatusCode::BAD_GATEWAY,
                    "chime_error",
                    format!("the Chime API rejected the call: {why}"),
                )
            }
        };
        (status, Json(json!({ "error": error, "message": message }))).into_response()
    }
}

/// `POST /private/meetings` — create a meeting and hand back its code.
pub async fn create_meeting(
    Extension(chime): Extension<ChimeState>,
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, ChimeError> {
    let meeting = chime
        .client()?
        .create_meeting()
        .client_request_token(unique_id("req"))
        .external_meeting_id(unique_id("omoffice"))
        .media_region(&chime.media_region)
        .send()
        .await
        .map_err(upstream)?
        .meeting
        .ok_or_else(|| ChimeError::Upstream("CreateMeeting returned no meeting".to_string()))?;

    let code = meeting.meeting_id().unwrap_or_default().to_string();

    // Recorded before the code is handed back, and a failure fails the call:
    // a meeting missing from the history is worse than one nobody joined, and
    // Chime drops an empty meeting on its own.
    db.record_meeting(&code, &claims.sub).await?;
    tracing::info!("meeting {code} created by {}", claims.sub);

    Ok(Json(json!({
        "code": code,
        "created_by": claims.sub,
        "Meeting": meeting_json(&meeting),
    })))
}

/// `POST /private/meetings/{code}/attendees` — join the meeting named by `code`.
///
/// Returns both halves the browser SDK needs: the meeting it looked up and the
/// attendee it just created for the caller.
pub async fn join_meeting(
    Extension(chime): Extension<ChimeState>,
    Extension(claims): Extension<Claims>,
    Path(code): Path<String>,
) -> Result<Json<Value>, ChimeError> {
    let client = chime.client()?;

    let meeting = client
        .get_meeting()
        .meeting_id(&code)
        .send()
        .await
        .map_err(|err| {
            if err
                .as_service_error()
                .is_some_and(|e| e.is_not_found_exception())
            {
                ChimeError::MeetingNotFound
            } else {
                upstream(err)
            }
        })?
        .meeting
        .ok_or(ChimeError::MeetingNotFound)?;

    let attendee = client
        .create_attendee()
        .meeting_id(&code)
        // Unique per join: the same person may be in the meeting from two tabs.
        .external_user_id(external_user_id(&claims.sub))
        .send()
        .await
        .map_err(upstream)?
        .attendee
        .ok_or_else(|| ChimeError::Upstream("CreateAttendee returned no attendee".to_string()))?;

    tracing::info!("{} joined meeting {code}", claims.sub);

    Ok(Json(json!({
        "code": code,
        "Meeting": meeting_json(&meeting),
        "Attendee": attendee_json(&attendee),
    })))
}

fn upstream<E, R>(err: aws_sdk_chimesdkmeetings::error::SdkError<E, R>) -> ChimeError
where
    E: std::error::Error + 'static,
    R: std::fmt::Debug,
{
    ChimeError::Upstream(
        aws_sdk_chimesdkmeetings::error::DisplayErrorContext(&err)
            .to_string()
            .replace('\n', " "),
    )
}

/// Chime wants a 2-64 char identifier that is unique per call.
fn unique_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{nanos}-{seq}")
}

/// The attendee identity the other participants see. Chime caps it at 64
/// chars, so a long email loses its tail rather than the call failing.
fn external_user_id(email: &str) -> String {
    let suffix = unique_id("j");
    let room = 64usize.saturating_sub(suffix.len() + 1);
    let head: String = email.chars().take(room).collect();
    format!("{head}#{suffix}")
}

/// The `Meeting` shape `MeetingSessionConfiguration` reads in the browser SDK.
fn meeting_json(meeting: &Meeting) -> Value {
    let placement = meeting.media_placement();
    json!({
        "MeetingId": meeting.meeting_id(),
        "ExternalMeetingId": meeting.external_meeting_id(),
        "MediaRegion": meeting.media_region(),
        "MediaPlacement": {
            "AudioHostUrl": placement.and_then(|p| p.audio_host_url()),
            "AudioFallbackUrl": placement.and_then(|p| p.audio_fallback_url()),
            "SignalingUrl": placement.and_then(|p| p.signaling_url()),
            "TurnControlUrl": placement.and_then(|p| p.turn_control_url()),
            "ScreenDataUrl": placement.and_then(|p| p.screen_data_url()),
            "ScreenSharingUrl": placement.and_then(|p| p.screen_sharing_url()),
            "ScreenViewingUrl": placement.and_then(|p| p.screen_viewing_url()),
            "EventIngestionUrl": placement.and_then(|p| p.event_ingestion_url()),
        },
    })
}

fn attendee_json(attendee: &Attendee) -> Value {
    json!({
        "AttendeeId": attendee.attendee_id(),
        "ExternalUserId": attendee.external_user_id(),
        "JoinToken": attendee.join_token(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_ids_do_not_repeat() {
        assert_ne!(unique_id("req"), unique_id("req"));
    }

    #[test]
    fn a_long_email_still_fits_chimes_64_char_limit() {
        let email = format!("{}@example.com", "a".repeat(120));
        assert!(external_user_id(&email).len() <= 64);
    }
}
