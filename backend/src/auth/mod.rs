//! JWT bearer auth. Policy (protected prefixes, allowed emails) is decided
//! by `app()` via [`AuthConfig`]; [`keys`] holds the signing key.

use std::time::Duration;

use axum::{
    extract::{Extension, Path, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::Algorithm;
#[cfg(test)]
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::db::Db;
use crate::users;

mod keys;
pub use keys::{AuthState, KeySource};

/// How long an issued token is valid.
const TOKEN_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

const ALGORITHM: Algorithm = Algorithm::HS256;

/// JWT payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Email the token was issued to.
    pub sub: String,
    /// Issued at, unix seconds.
    pub iat: u64,
    /// Expiry, unix seconds.
    pub exp: u64,
}

/// Everything that can go wrong on the auth path. Each variant carries its own
/// status code and stable `error` slug.
#[derive(Debug)]
pub enum AuthError {
    MissingToken,
    InvalidToken,
    ExpiredToken,
    KeyUnavailable(String),
}

impl AuthError {
    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::MissingToken => (
                StatusCode::UNAUTHORIZED,
                "missing_token",
                "missing Authorization: Bearer <token> header".to_string(),
            ),
            Self::InvalidToken => (
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "the bearer token is malformed or was not signed by this API".to_string(),
            ),
            Self::ExpiredToken => (
                StatusCode::UNAUTHORIZED,
                "expired_token",
                "the bearer token has expired, request a new one".to_string(),
            ),
            Self::KeyUnavailable(why) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "key_unavailable",
                format!("could not load the signing key: {why}"),
            ),
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, error, message) = self.parts();
        if let Self::KeyUnavailable(why) = &self {
            tracing::error!("jwt key unavailable: {why}");
        }
        let body = Json(json!({ "error": error, "message": message }));

        if status == StatusCode::UNAUTHORIZED {
            // RFC 6750: 401 on a bearer-protected resource carries the challenge.
            let challenge = format!(r#"Bearer error="{error}""#);
            return (status, [(header::WWW_AUTHENTICATE, challenge)], body).into_response();
        }
        (status, body).into_response()
    }
}

pub struct AuthConfig {
    source: KeySource,
    protected: Vec<String>,
}

impl AuthConfig {
    pub fn new(source: KeySource) -> Self {
        Self {
            source,
            protected: Vec::new(),
        }
    }

    /// Route prefixes that require a bearer token. `"/private"` covers
    /// `/private` and everything under `/private/`, but not `/privateer`.
    pub fn protect<I, S>(mut self, prefixes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.protected = prefixes.into_iter().map(Into::into).collect();
        self
    }

    pub fn build(self) -> AuthState {
        keys::build(self.source, self.protected)
    }
}

/// `GET /auth/request-key/{email}` — also where a person first becomes a
/// row in `users`, the key every other table hangs off.
pub async fn request_key(
    State(state): State<AuthState>,
    Extension(db): Extension<Db>,
    Path(email): Path<String>,
) -> Result<Json<Value>, AuthError> {
    let email = email.trim().to_lowercase();
    let (token, expires_at) = state.issue(&email).await?;

    // Best effort on purpose: the token is already signed and valid, so a
    // database that is unreachable should not stop anyone getting one.
    if let Err(err) = users::record(&db, &email).await {
        tracing::warn!("could not record the user who asked for a key: {err:?}");
    }

    Ok(Json(json!({
        "token": token,
        "token_type": "Bearer",
        "email": email,
        "expires_at": expires_at,
        "expires_in": TOKEN_TTL.as_secs(),
    })))
}

/// Router-wide middleware: protected prefixes need a valid token, whose
/// [`Claims`] then reach the handler as an extension.
pub async fn require_bearer(
    State(state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Response {
    if !state.is_protected(request.uri().path()) {
        return next.run(request).await;
    }

    let token = match bearer_token(&request) {
        Some(token) => token,
        None => return AuthError::MissingToken.into_response(),
    };

    match state.verify(&token).await {
        Ok(claims) => {
            request.extensions_mut().insert(claims);
            next.run(request).await
        }
        Err(err) => err.into_response(),
    }
}

fn bearer_token(request: &Request) -> Option<String> {
    let value = request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .trim();
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

#[cfg(test)]
pub(crate) fn sign_for_test(key: &str, email: &str, exp: u64) -> String {
    let claims = Claims {
        sub: email.to_string(),
        iat: keys::unix_now(),
        exp,
    };
    encode(
        &Header::new(ALGORITHM),
        &claims,
        &EncodingKey::from_secret(key.as_bytes()),
    )
    .expect("test token signs")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AuthState {
        AuthConfig::new(KeySource::fixed("test-key"))
            .protect(vec!["/private"])
            .build()
    }

    #[test]
    fn protects_the_prefix_and_everything_under_it() {
        let state = state();
        assert!(state.is_protected("/private"));
        assert!(state.is_protected("/private/me"));
        assert!(state.is_protected("/private/a/b"));
    }

    #[test]
    fn leaves_lookalike_paths_public() {
        let state = state();
        assert!(!state.is_protected("/"));
        assert!(!state.is_protected("/privateer"));
        assert!(!state.is_protected("/public/private"));
    }

    #[tokio::test]
    async fn issued_tokens_verify() {
        let state = state();
        let (token, exp) = state.issue("dev@omoffice.example").await.unwrap();
        let claims = state.verify(&token).await.unwrap();
        assert_eq!(claims.sub, "dev@omoffice.example");
        assert_eq!(claims.exp, exp);
        assert_eq!(exp - claims.iat, TOKEN_TTL.as_secs());
    }
}
