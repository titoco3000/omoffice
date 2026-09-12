//! JWT bearer auth, and the one public route: a magic-link request. This
//! module only decides *who* someone is; [`crate::core::rbac::require_role`]
//! and `require_member` decide what they're allowed to do once they are.
//!
//! There is no route that hands a token back directly. `request_key`
//! registers the caller with `rbac` (creating their `users` row on first
//! contact), mints a token, and emails it -- the mailbox is the only way to
//! redeem it.

use axum::extract::{Extension, Path, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{http::StatusCode, routing::get, Json, Router};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::core::communication::email;
use crate::core::data::db::Db;
use crate::core::rbac;

const ALGORITHM: Algorithm = Algorithm::HS256;
const TOKEN_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;

/// JWT payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// The `users.id` the token was issued to.
    pub sub: String,
    pub exp: u64,
}

#[derive(Debug)]
pub enum AuthError {
    MissingToken,
    InvalidToken,
    ExpiredToken,
    Rbac(rbac::RbacError),
}

impl From<rbac::RbacError> for AuthError {
    fn from(err: rbac::RbacError) -> Self {
        Self::Rbac(err)
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            Self::MissingToken => (StatusCode::UNAUTHORIZED, "missing_token"),
            Self::InvalidToken => (StatusCode::UNAUTHORIZED, "invalid_token"),
            Self::ExpiredToken => (StatusCode::UNAUTHORIZED, "expired_token"),
            Self::Rbac(_) => (StatusCode::INTERNAL_SERVER_ERROR, "rbac_error"),
        };
        (status, Json(json!({ "error": error }))).into_response()
    }
}

/// `JWT_KEY`, or an insecure fallback for local development. No key
/// rotation and no Secrets Manager fetch yet.
fn signing_key() -> String {
    std::env::var("JWT_KEY").unwrap_or_else(|_| "insecure-local-development-key".to_string())
}

fn issue(user_id: &str) -> Result<String, AuthError> {
    let claims = Claims {
        sub: user_id.to_string(),
        exp: now() + TOKEN_TTL_SECONDS,
    };
    encode(
        &Header::new(ALGORITHM),
        &claims,
        &EncodingKey::from_secret(signing_key().as_bytes()),
    )
    .map_err(|_| AuthError::InvalidToken)
}

/// Verify a bearer token into the [`Claims`] a protected route reads.
pub async fn verify(token: &str) -> Result<Claims, AuthError> {
    let mut validation = Validation::new(ALGORITHM);
    validation.set_required_spec_claims(&["exp", "sub"]);
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(signing_key().as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|err| match err.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::ExpiredToken,
        _ => AuthError::InvalidToken,
    })
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_secs()
}

/// `GET /auth/request-key/{email}` -- also where a person first becomes a
/// `users` row, via [`rbac::register_user`].
async fn request_key(
    Extension(db): Extension<Db>,
    Path(email_addr): Path<String>,
) -> Result<Json<serde_json::Value>, AuthError> {
    let email_addr = email_addr.trim().to_lowercase();
    let user_id = rbac::register_user(&db, &email_addr, None).await?;
    let token = issue(&user_id)?;
    let magic_link = format!("{}/?login={token}", crate::core::frontend_base_url());
    email::send_email(
        "noreply@omoffice.example",
        &email_addr,
        "Your sign-in link",
        &magic_link,
    );
    Ok(Json(json!({ "sent_to": email_addr })))
}

/// Middleware for a protected route: rejects a missing or invalid bearer
/// token, else inserts [`Claims`] as an extension for the handler to read.
pub async fn require_bearer(mut request: Request, next: Next) -> Response {
    let token = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string);

    let Some(token) = token else {
        return AuthError::MissingToken.into_response();
    };

    match verify(&token).await {
        Ok(claims) => {
            request.extensions_mut().insert(claims);
            next.run(request).await
        }
        Err(err) => err.into_response(),
    }
}

/// `/auth/*` -- just the one public route.
pub fn routes() -> Router {
    Router::new().route("/request-key/{email}", get(request_key))
}
