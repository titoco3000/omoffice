//! Walking into someone on the office map starts or joins a meeting with
//! them, no button involved. Whoever moved works out who's within
//! [`PROXIMITY_RANGE`] from the live positions its tab already renders (the
//! stored ones can lag a second behind, see `MoveBody::persist`) and names
//! them to [`route`]; this module decides what that means against the
//! meetings as they actually stand in the database -- not the caller tab's
//! own copy of the meeting list, which can lag behind a force-join or a
//! merge that just happened to it. Meetings themselves are only reached
//! through `features::meetings`' public API, same as `core::rbac` is.
//!
//! The rules, from the caller's point of view:
//! - Nobody nearby is in a meeting, and neither is the caller: a new
//!   meeting is created for all of them.
//! - The caller is in a meeting and someone nearby isn't: they're pulled
//!   into the caller's meeting.
//! - Someone nearby is in a different meeting: the caller's whole meeting
//!   (or just the caller, if they're in none) moves into theirs, along with
//!   anyone else nearby who's in no meeting.
//!
//! A locked (`invite_only`) meeting is never a target: walking past
//! someone in one doesn't drag anybody in, or them out. The caller's own
//! meeting is the exception -- the caller is already an attendee, and
//! pulling someone in is theirs to do, same as sending an invite.
//!
//! All of that takes a while -- a room to claim, a Chime attendee to mint
//! per person -- so nobody waits on it to see the meeting. The caller's tab
//! guesses the outcome from its own copy of the meeting list and opens the
//! meeting straight away, proposing the id for a new one (`meeting_id` in
//! [`ProximityBody`]) so its guess and the backend's answer name the same
//! meeting. Everyone else about to be moved is told over [`PENDING_CHANNEL`]
//! as soon as [`gather`] has decided, before any of the slow part.
//!
//! Walking *out* of range is not handled here: whether the caller just left
//! someone behind depends on where they were a step ago, which only the
//! caller's tab knows -- it calls `POST /meeting/{id}/leave` itself.

use std::collections::{HashMap, HashSet};

use axum::extract::{Extension, Path};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::MapError;
use crate::core::auth::Claims;
use crate::core::chime::ChimeState;
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::Db;
use crate::core::rbac;
use crate::features::meetings::{self, MeetingError};

/// How many cells away, in each direction, someone counts as nearby -- a
/// `(2 * PROXIMITY_RANGE + 1)`-cell square centered on the mover. Enforced
/// by the frontend, which is where live positions are; see the module doc.
pub const PROXIMITY_RANGE: i32 = 2;

/// `{"org_id", "meeting_id"}`, pushed to whoever [`gather`] is about to move
/// into `meeting_id`, ahead of the forced join itself -- see the module doc.
pub const PENDING_CHANNEL: web_messenger::Channel = "map_meeting_pending";

/// Apply the module doc's rules for `caller` having ended up within range
/// of `nearby` (already narrowed to online members of `org_id`, caller
/// excluded). Everyone moved other than the caller is pushed a forced join
/// over [`meetings::FORCE_CHANNEL`], same as [`meetings::force_join`]; the
/// caller gets the same join response [`meetings::join`] would return, or
/// `{"meeting_id", "unchanged": true}` when they stay where they are
/// (`meeting_id` then being `null` if that's no meeting at all).
async fn gather(
    db: &Db,
    chime: &ChimeState,
    messenger: &Messenger,
    org_id: &str,
    caller: &str,
    nearby: &[String],
    proposed_id: Option<&str>,
) -> Result<Value, MeetingError> {
    let mut everyone = nearby.to_vec();
    everyone.push(caller.to_string());
    let attending = meetings::current_meetings(db, org_id, &everyone).await?;
    let mine = attending.get(caller).map(|meeting| meeting.id.clone());

    // The unlocked meeting, other than the caller's own, that the most
    // nearby people are in -- ties broken by id, so two callers racing over
    // the same crowd at least agree on where it should end up.
    let mut others: HashMap<&str, usize> = HashMap::new();
    for user in nearby {
        if let Some(meeting) = attending.get(user) {
            if !meeting.invite_only && Some(&meeting.id) != mine.as_ref() {
                *others.entry(meeting.id.as_str()).or_default() += 1;
            }
        }
    }
    let theirs = others
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(id, _)| id.to_string());

    let free: Vec<String> = nearby
        .iter()
        .filter(|user| !attending.contains_key(*user))
        .cloned()
        .collect();

    // `new`: the target doesn't exist yet -- created below, only once
    // everyone moving into it has been told.
    let (target, new) = match (theirs, &mine) {
        (Some(theirs), _) => (theirs, false),
        (None, Some(mine)) if !free.is_empty() => (mine.clone(), false),
        // Nothing to join and nobody to pull in: everyone nearby is either
        // already in the caller's meeting, or in a locked one.
        (None, _) if free.is_empty() => {
            return Ok(json!({ "meeting_id": mine, "unchanged": true }));
        }
        (None, _) => (
            proposed_id
                .map(str::to_string)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            true,
        ),
    };

    // The caller's own meeting comes along whole when it's merging into
    // someone else's.
    let mut movers: Vec<String> = Vec::new();
    if let Some(mine) = mine.as_deref().filter(|mine| *mine != target) {
        movers.extend(meetings::attendee_ids(db, mine).await?);
    }
    movers.extend(free);
    let mut seen = HashSet::new();
    movers.retain(|user| user != caller && seen.insert(user.clone()));

    messenger
        .send(
            PENDING_CHANNEL,
            &movers,
            json!({ "org_id": org_id, "meeting_id": target }),
        )
        .await;
    if new {
        meetings::create_with_id(db, &target, caller, Some(org_id), false).await?;
    }

    for user in &movers {
        match meetings::move_into(db, chime, messenger, &target, user).await {
            Ok(response) => {
                messenger
                    .send(
                        meetings::FORCE_CHANNEL,
                        std::slice::from_ref(user),
                        response,
                    )
                    .await
            }
            Err(err) => {
                tracing::warn!("map: could not move {user} into {target}: {err:?}")
            }
        }
    }

    if mine.as_deref() == Some(target.as_str()) {
        return Ok(json!({ "meeting_id": target, "unchanged": true }));
    }
    // Last, so the caller's Chime attendee is minted in whatever room the
    // movers above ended up claiming, rather than one of them replacing it.
    meetings::move_into(db, chime, messenger, &target, caller).await
}

#[derive(Deserialize)]
pub(super) struct ProximityBody {
    /// Who the caller's tab sees within [`PROXIMITY_RANGE`] of them.
    nearby: Vec<String>,
    /// The id to give the meeting, should this end up creating one -- the
    /// one the caller's tab already opened optimistically. Ignored unless
    /// it's a well-formed UUID, same shape [`meetings::create`] would mint.
    #[serde(default)]
    meeting_id: Option<String>,
}

/// `POST /map/org/{org_id}/proximity` -- the caller just moved within range
/// of `nearby`: start, join or merge meetings with them, see [`gather`].
/// `nearby` is taken at the frontend's word, narrowed to online members
/// other than the caller -- no more trust than `POST /meeting/{id}/force`
/// already extends to dragging someone into a meeting.
pub(super) async fn route(
    Extension(db): Extension<Db>,
    Extension(chime): Extension<ChimeState>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(body): Json<ProximityBody>,
) -> Result<Json<Value>, MapError> {
    let (_, online) = tokio::try_join!(
        rbac::require_member(&db, &org_id, &claims.sub),
        rbac::online_member_ids(&db, &org_id),
    )?;
    let online: HashSet<String> = online.into_iter().collect();
    let nearby: Vec<String> = body
        .nearby
        .into_iter()
        .filter(|id| *id != claims.sub && online.contains(id))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let proposed_id = body
        .meeting_id
        .filter(|id| uuid::Uuid::parse_str(id).is_ok());
    let response = gather(
        &db,
        &chime,
        &messenger,
        &org_id,
        &claims.sub,
        &nearby,
        proposed_id.as_deref(),
    )
    .await?;
    Ok(Json(response))
}
