//! Live map moves over AppSync Events, off this Lambda's hot path: each
//! client publishes its own moves straight to AppSync and subscribes to
//! everyone else's, and this module only hands out the credentials for
//! that (see [`route`]) and decides what they allow (see [`authorize`], run
//! by the separate `map-authorizer` Lambda, `backend/map_authorizer.rs`).
//! Persistence still goes through `POST /map/org/{org_id}/move`.
//!
//! Moves are delivered by region, so what a member receives (and what
//! AppSync bills) doesn't grow with the size of the org. The office is cut
//! into [`REGION_SIZE`](super::REGION_SIZE)-cell squares, and there are two
//! kinds of channel:
//! - fine, `/map/{org_id}/{rx}-{ry}/{user_id}`: published by a member while
//!   their cell is in region `(rx, ry)`, and subscribed to as
//!   `/map/{org_id}/{rx}-{ry}/*` by whoever has that region in view;
//! - coarse, `/map/{org_id}/c/{user_id}`: published (at a lower rate) by
//!   every moving member, and subscribed to as `/map/{org_id}/c/*` by
//!   those zoomed out too far to subscribe region by region.
//!
//! A move is a `{"x", "y", "facing"}` payload. Who moved comes from the
//! last path segment, never the payload -- the `map` namespace's
//! `onPublish` handler (`infra/map_events.tf`) stamps it on as `user_id`,
//! since a subscriber's data frame carries no channel of its own.
//!
//! With [`ENDPOINT_ENV`] unset (a local run, no AppSync to talk to),
//! [`route`] says so, and the frontend falls back to posting every move and
//! receiving [`super::MOVE_CHANNEL`] over the messenger instead.

use axum::extract::{Extension, Path};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{MapError, REGIONS_PER_SIDE};
use crate::core::auth::{self, Claims};
use crate::core::data::db::Db;
use crate::core::rbac;

/// The AppSync Events API's HTTP host (not the realtime one: the frontend
/// derives that, and needs this one as the `host` its auth header names).
pub const ENDPOINT_ENV: &str = "MAP_EVENTS_ENDPOINT";

/// The channel namespace `infra/map_events.tf` declares.
const NAMESPACE: &str = "map";

pub fn endpoint() -> Option<String> {
    std::env::var(ENDPOINT_ENV)
        .ok()
        .filter(|endpoint| !endpoint.is_empty())
}

/// `GET /map/org/{org_id}/realtime` -- `{"endpoint", "token"}` for
/// connecting to the map's AppSync Events API as the caller in `org_id`,
/// both `null` when there is none (see the module doc). The token lasts
/// [`auth::MAP_TOKEN_TTL_SECONDS`]; the frontend fetches a new one before
/// then and reconnects with it.
pub(super) async fn route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, MapError> {
    rbac::require_member(&db, &org_id, &claims.sub).await?;
    let Some(endpoint) = endpoint() else {
        return Ok(Json(json!({ "endpoint": null, "token": null })));
    };
    let token = auth::issue_map_token(&claims.sub, &org_id)?;
    Ok(Json(json!({ "endpoint": endpoint, "token": token })))
}

/// What AppSync hands a Lambda authorizer, narrowed to what [`authorize`]
/// reads.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizerRequest {
    #[serde(default)]
    pub authorization_token: String,
    pub request_context: AuthorizerContext,
}

#[derive(Debug, Deserialize)]
pub struct AuthorizerContext {
    /// `EVENT_CONNECT`, `EVENT_SUBSCRIBE` or `EVENT_PUBLISH`.
    pub operation: String,
    /// `None` on `EVENT_CONNECT`.
    #[serde(default)]
    pub channel: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizerResponse {
    pub is_authorized: bool,
    /// Seconds AppSync may cache this answer for. AppSync keys that cache
    /// on token, operation and channel together, so a publish pays for the
    /// authorizer once per token, not once per move.
    pub ttl_override: u64,
}

impl AuthorizerResponse {
    fn denied() -> Self {
        Self {
            is_authorized: false,
            ttl_override: 0,
        }
    }
}

/// The coarse channel's region segment.
const COARSE: &str = "c";

/// `c`, or `{rx}-{ry}` with both in `0..REGIONS_PER_SIDE`.
fn valid_region(region: &str) -> bool {
    if region == COARSE {
        return true;
    }
    let Some((rx, ry)) = region.split_once('-') else {
        return false;
    };
    let index = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && part
                .parse::<i32>()
                .is_ok_and(|n| (0..REGIONS_PER_SIDE).contains(&n))
    };
    index(rx) && index(ry)
}

/// Decide one AppSync authorizer request: the token must be a valid map
/// token (signature and expiry, nothing else -- see
/// [`auth::verify_map_token`]), and then
/// - connect: allowed, the channel checks come per operation;
/// - subscribe: `/map/{org_id}/{region}/*` only;
/// - publish: `/map/{org_id}/{region}/{sub}` only -- a member's own channel;
///
/// where `region` is `c` or a valid `{rx}-{ry}` (see [`valid_region`]).
///
/// An allowed answer is cached for the token's remaining lifetime, so it
/// never outlives the token itself.
pub fn authorize(request: &AuthorizerRequest, now: u64) -> AuthorizerResponse {
    let Ok(claims) = auth::verify_map_token(&request.authorization_token) else {
        return AuthorizerResponse::denied();
    };
    let channel = request.request_context.channel.as_deref().map(|channel| {
        channel
            .trim_start_matches('/')
            .trim_end_matches('/')
            .split('/')
            .collect::<Vec<_>>()
    });
    let allowed = match (
        request.request_context.operation.as_str(),
        channel.as_deref(),
    ) {
        ("EVENT_CONNECT", _) => true,
        ("EVENT_SUBSCRIBE", Some([namespace, org_id, region, "*"])) => {
            *namespace == NAMESPACE && *org_id == claims.org_id && valid_region(region)
        }
        ("EVENT_PUBLISH", Some([namespace, org_id, region, user_id])) => {
            *namespace == NAMESPACE
                && *org_id == claims.org_id
                && valid_region(region)
                && *user_id == claims.sub
        }
        _ => false,
    };
    if !allowed {
        return AuthorizerResponse::denied();
    }
    AuthorizerResponse {
        is_authorized: true,
        ttl_override: claims.exp.saturating_sub(now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(token: &str, operation: &str, channel: Option<&str>) -> AuthorizerRequest {
        serde_json::from_value(json!({
            "authorizationToken": token,
            "requestContext": {
                "apiId": "api",
                "accountId": "111122223333",
                "requestId": "request",
                "operation": operation,
                "channelNamespaceName": channel.map(|_| NAMESPACE),
                "channel": channel,
            },
            "requestHeaders": {},
        }))
        .expect("the shape AppSync documents")
    }

    fn allowed(token: &str, operation: &str, channel: Option<&str>) -> bool {
        authorize(&request(token, operation, channel), auth::now()).is_authorized
    }

    #[test]
    fn a_member_publishes_to_their_own_channels_only() {
        let token = auth::issue_map_token("alice", "org-1").unwrap();
        assert!(allowed(
            &token,
            "EVENT_PUBLISH",
            Some("/map/org-1/3-4/alice")
        ));
        assert!(allowed(
            &token,
            "EVENT_PUBLISH",
            Some("/map/org-1/0-9/alice")
        ));
        assert!(allowed(&token, "EVENT_PUBLISH", Some("/map/org-1/c/alice")));
        assert!(!allowed(
            &token,
            "EVENT_PUBLISH",
            Some("/map/org-1/3-4/bob")
        ));
        assert!(!allowed(&token, "EVENT_PUBLISH", Some("/map/org-1/c/bob")));
        assert!(!allowed(
            &token,
            "EVENT_PUBLISH",
            Some("/map/org-2/3-4/alice")
        ));
        assert!(!allowed(&token, "EVENT_PUBLISH", Some("/map/org-1/3-4/*")));
        assert!(!allowed(
            &token,
            "EVENT_PUBLISH",
            Some("/other/org-1/3-4/alice")
        ));
    }

    #[test]
    fn publishing_needs_a_valid_region() {
        let token = auth::issue_map_token("alice", "org-1").unwrap();
        for region in [
            "10-0", "0-10", "-1-0", "a-b", "1-2-3", "", "-", "1-", "+1-0",
        ] {
            let channel = format!("/map/org-1/{region}/alice");
            assert!(
                !allowed(&token, "EVENT_PUBLISH", Some(&channel)),
                "{region}"
            );
            let channel = format!("/map/org-1/{region}/*");
            assert!(
                !allowed(&token, "EVENT_SUBSCRIBE", Some(&channel)),
                "{region}"
            );
        }
    }

    #[test]
    fn the_old_three_segment_channels_are_denied() {
        let token = auth::issue_map_token("alice", "org-1").unwrap();
        assert!(!allowed(&token, "EVENT_PUBLISH", Some("/map/org-1/alice")));
        assert!(!allowed(&token, "EVENT_SUBSCRIBE", Some("/map/org-1/*")));
    }

    #[test]
    fn a_member_subscribes_to_their_orgs_regions_only() {
        let token = auth::issue_map_token("alice", "org-1").unwrap();
        assert!(allowed(&token, "EVENT_SUBSCRIBE", Some("/map/org-1/3-4/*")));
        assert!(allowed(&token, "EVENT_SUBSCRIBE", Some("/map/org-1/c/*")));
        assert!(!allowed(
            &token,
            "EVENT_SUBSCRIBE",
            Some("/map/org-2/3-4/*")
        ));
        assert!(!allowed(&token, "EVENT_SUBSCRIBE", Some("/map/org-2/c/*")));
        assert!(!allowed(
            &token,
            "EVENT_SUBSCRIBE",
            Some("/map/org-1/3-4/bob")
        ));
        assert!(!allowed(&token, "EVENT_SUBSCRIBE", Some("/map/org-1/*")));
        assert!(!allowed(&token, "EVENT_SUBSCRIBE", Some("/map/*")));
    }

    #[test]
    fn connect_needs_a_valid_map_token() {
        let token = auth::issue_map_token("alice", "org-1").unwrap();
        assert!(allowed(&token, "EVENT_CONNECT", None));
        assert!(!allowed("not-a-token", "EVENT_CONNECT", None));
        let bearer = auth::issue("alice").unwrap();
        assert!(!allowed(&bearer, "EVENT_CONNECT", None));
    }

    #[test]
    fn an_allowed_answer_is_cached_no_longer_than_the_token_lives() {
        let token = auth::issue_map_token("alice", "org-1").unwrap();
        let response = authorize(
            &request(&token, "EVENT_PUBLISH", Some("/map/org-1/3-4/alice")),
            auth::now(),
        );
        assert!(response.ttl_override <= auth::MAP_TOKEN_TTL_SECONDS);
        assert!(response.ttl_override > 0);
    }
}
