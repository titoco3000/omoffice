//! Live map moves over AppSync Events, off this Lambda's hot path: each
//! client publishes its own moves straight to AppSync and subscribes to
//! everyone else's, and this module only hands out the credentials for
//! that (see [`route`]) and decides what they allow (see [`authorize`], run
//! by the separate `map-authorizer` Lambda, `backend/map_authorizer.rs`).
//! Persistence still goes through `POST /map/org/{org_id}/move`.
//!
//! One channel per member per org, `/map/{org_id}/{user_id}`: a member
//! publishes `{"x", "y", "facing"}` to their own and subscribes to `/map/{org_id}/*`.
//! Who moved comes from the channel path, never the payload -- the `map`
//! namespace's `onPublish` handler (`infra/map_events.tf`) stamps it on as
//! `user_id`, since a subscriber's data frame carries no channel of its own.
//!
//! With [`ENDPOINT_ENV`] unset (a local run, no AppSync to talk to),
//! [`route`] says so, and the frontend falls back to posting every move and
//! receiving [`super::MOVE_CHANNEL`] over the messenger instead.

use axum::extract::{Extension, Path};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::MapError;
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

/// Decide one AppSync authorizer request: the token must be a valid map
/// token (signature and expiry, nothing else -- see
/// [`auth::verify_map_token`]), and then
/// - connect: allowed, the channel checks come per operation;
/// - subscribe: `/map/{org_id}/*` only;
/// - publish: `/map/{org_id}/{sub}` only -- a member's own channel.
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
        ("EVENT_SUBSCRIBE", Some([namespace, org_id, "*"])) => {
            *namespace == NAMESPACE && *org_id == claims.org_id
        }
        ("EVENT_PUBLISH", Some([namespace, org_id, user_id])) => {
            *namespace == NAMESPACE && *org_id == claims.org_id && *user_id == claims.sub
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
    fn a_member_publishes_to_their_own_channel_only() {
        let token = auth::issue_map_token("alice", "org-1").unwrap();
        assert!(allowed(&token, "EVENT_PUBLISH", Some("/map/org-1/alice")));
        assert!(!allowed(&token, "EVENT_PUBLISH", Some("/map/org-1/bob")));
        assert!(!allowed(&token, "EVENT_PUBLISH", Some("/map/org-2/alice")));
        assert!(!allowed(&token, "EVENT_PUBLISH", Some("/map/org-1/*")));
        assert!(!allowed(
            &token,
            "EVENT_PUBLISH",
            Some("/other/org-1/alice")
        ));
    }

    #[test]
    fn a_member_subscribes_to_their_org_wildcard_only() {
        let token = auth::issue_map_token("alice", "org-1").unwrap();
        assert!(allowed(&token, "EVENT_SUBSCRIBE", Some("/map/org-1/*")));
        assert!(!allowed(&token, "EVENT_SUBSCRIBE", Some("/map/org-2/*")));
        assert!(!allowed(&token, "EVENT_SUBSCRIBE", Some("/map/org-1/bob")));
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
            &request(&token, "EVENT_PUBLISH", Some("/map/org-1/alice")),
            auth::now(),
        );
        assert!(response.ttl_override <= auth::MAP_TOKEN_TTL_SECONDS);
        assert!(response.ttl_override > 0);
    }
}
