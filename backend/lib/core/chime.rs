//! Amazon Chime SDK meetings -- the client, and nothing else. No routes, no
//! database, no idea which of our own meetings a Chime session backs.
//!
//! [`ChimeEvent`]s let interested code react to something Chime reports
//! (a room emptying out) instead of polling occupancy itself: [`on_event`]
//! subscribes, [`publish`] fires. Nothing here decides *when* an event
//! fires -- that happens once Chime's own notifications are wired in (an
//! EventBridge rule targeting this lambda is the plan; [`handle_event_bridge`]
//! is the seam for it), not anything in this module itself.

use std::sync::{Arc, RwLock};

use aws_sdk_chimesdkmeetings::types::{Attendee, Meeting};
use aws_sdk_chimesdkmeetings::Client;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

const DEFAULT_MEDIA_REGION: &str = "us-east-1";

/// Something Chime told us happened, past the point our own bookkeeping
/// would ever notice on its own.
#[derive(Debug, Clone)]
pub enum ChimeEvent {
    /// Every attendee has left `meeting_id`; Chime will drop the meeting
    /// itself shortly after.
    MeetingEmptied { meeting_id: String },
}

type EventHandler = Arc<dyn Fn(ChimeEvent) + Send + Sync>;

/// Chime client, the region meetings are created in, and the registry of
/// [`ChimeEvent`] subscribers. Cheap to clone; the client is `None` in
/// tests, where the routes answer 503 instead of calling AWS.
#[derive(Clone)]
pub struct ChimeState {
    client: Option<Client>,
    media_region: String,
    handlers: Arc<RwLock<Vec<EventHandler>>>,
}

impl ChimeState {
    #[cfg(test)]
    pub fn disabled() -> Self {
        Self {
            client: None,
            media_region: DEFAULT_MEDIA_REGION.to_string(),
            handlers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Build the client from the ambient AWS config. Media region is
    /// `CHIME_MEDIA_REGION`, else the SDK's region, else `us-east-1`.
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
            handlers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn client(&self) -> Result<&Client, ChimeError> {
        self.client.as_ref().ok_or(ChimeError::Unavailable)
    }

    /// Register to hear every [`ChimeEvent`]. Meant to be called once per
    /// interested party, at startup.
    pub fn on_event<F>(&self, handler: F)
    where
        F: Fn(ChimeEvent) + Send + Sync + 'static,
    {
        self.handlers
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .push(Arc::new(handler));
    }

    fn publish(&self, event: ChimeEvent) {
        for handler in self
            .handlers
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
        {
            handler(event.clone());
        }
    }

    /// Feed one EventBridge notification in, translate it to a [`ChimeEvent`]
    /// if it's one we care about, and [`publish`](Self::publish) it. Not
    /// wired to a route yet -- this is the seam a future webhook route calls into.
    pub fn handle_event_bridge(&self, _payload: &Value) {
        todo!("parse a chime::MeetingEnded/AttendeeLeft detail and self.publish(...) accordingly")
    }

    /// Create a meeting. Its `MeetingId` is what the rest of the code calls
    /// the room id.
    pub async fn create_meeting(&self) -> Result<Meeting, ChimeError> {
        self.client()?
            .create_meeting()
            .client_request_token(unique_id("req"))
            .external_meeting_id(unique_id("omoffice"))
            .media_region(&self.media_region)
            .send()
            .await
            .map_err(upstream)?
            .meeting
            .ok_or_else(|| ChimeError::Upstream("CreateMeeting returned no meeting".to_string()))
    }

    /// Look a meeting up by id. Chime drops a meeting once it empties, so a
    /// stale id is [`ChimeError::MeetingNotFound`], not a failure.
    pub async fn get_meeting(&self, meeting_id: &str) -> Result<Meeting, ChimeError> {
        self.client()?
            .get_meeting()
            .meeting_id(meeting_id)
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
            .ok_or(ChimeError::MeetingNotFound)
    }

    /// Mint an attendee for a meeting. `external_user_id` is the identity
    /// the other participants see -- [`external_user_id`] builds one that fits.
    pub async fn create_attendee(
        &self,
        meeting_id: &str,
        external_user_id: &str,
    ) -> Result<Attendee, ChimeError> {
        self.client()?
            .create_attendee()
            .meeting_id(meeting_id)
            .external_user_id(external_user_id)
            .send()
            .await
            .map_err(upstream)?
            .attendee
            .ok_or_else(|| ChimeError::Upstream("CreateAttendee returned no attendee".to_string()))
    }

    /// Tear a meeting down explicitly, rather than waiting for Chime's own
    /// idle timeout to notice nobody is left.
    pub async fn end_meeting(&self, meeting_id: &str) -> Result<(), ChimeError> {
        self.client()?
            .delete_meeting()
            .meeting_id(meeting_id)
            .send()
            .await
            .map_err(upstream)?;
        Ok(())
    }
}

/// What the routes can fail with.
#[derive(Debug)]
pub enum ChimeError {
    /// No Chime client was built.
    Unavailable,
    /// The id does not name a live meeting. Chime drops empty meetings, so
    /// this is also what a stale id looks like.
    MeetingNotFound,
    /// Anything the Chime API itself rejected.
    Upstream(String),
}

impl IntoResponse for ChimeError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "chime_unavailable",
                "this API instance has no Chime client configured".to_string(),
            ),
            Self::MeetingNotFound => (
                StatusCode::NOT_FOUND,
                "meeting_not_found",
                "no live Chime meeting has that id; it may have ended".to_string(),
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{nanos}-{seq}")
}

/// An attendee identity the other participants see, unique per join. Chime
/// caps it at 64 chars, so a long label loses its tail, not the call.
pub fn external_user_id(label: &str) -> String {
    let suffix = unique_id("j");
    let room = 64usize.saturating_sub(suffix.len() + 1);
    let head: String = label.chars().take(room).collect();
    format!("{head}#{suffix}")
}

/// The `Meeting` shape `MeetingSessionConfiguration` reads in the browser SDK.
pub fn meeting_json(meeting: &Meeting) -> Value {
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

pub fn attendee_json(attendee: &Attendee) -> Value {
    json!({
        "AttendeeId": attendee.attendee_id(),
        "ExternalUserId": attendee.external_user_id(),
        "JoinToken": attendee.join_token(),
    })
}
