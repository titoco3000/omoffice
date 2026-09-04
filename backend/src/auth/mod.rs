//! JWT bearer auth.
//!
//! The policy — which prefixes are protected, which emails may ask for a token —
//! is not decided here; `app()` states it and passes it in via [`AuthConfig`].
//! This module only implements it:
//!
//! * [`KeySource`] — where the HMAC key comes from. In AWS it lives in Secrets
//!   Manager and rotates every Sunday, so tokens are verified against both the
//!   current and the previous version: a token issued on Saturday still works on
//!   Monday.
//! * [`request_key`] — hands a 48h token to a whitelisted email.
//! * [`require_bearer`] — one middleware over the whole router that only demands
//!   a token for protected prefixes.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Extension, Path, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{
    decode, encode, errors::ErrorKind, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::db::Db;
use crate::users;

/// How long an issued token is valid.
const TOKEN_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How long a key fetched from Secrets Manager is reused before refetching.
/// Rotation is weekly, so a few minutes of staleness is harmless — and a token
/// signed with a just-rotated-out key still verifies against `AWSPREVIOUS`.
const KEY_CACHE_TTL: Duration = Duration::from_secs(300);

const ALGORITHM: Algorithm = Algorithm::HS256;

/// Key used when nothing is configured. Local development only — [`KeySource::from_env`]
/// refuses to hand it back inside Lambda.
const DEV_FALLBACK_KEY: &str = "insecure-local-development-key";

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
    EmailNotAllowed,
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
            Self::EmailNotAllowed => (
                StatusCode::FORBIDDEN,
                "email_not_allowed",
                "this email is not on the whitelist".to_string(),
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

/// Where the HMAC signing key comes from.
pub enum KeySource {
    /// A key held in memory: `JWT_KEY`, the local dev fallback, and tests.
    Fixed(String),
    /// Secrets Manager, rotated weekly by the infra rotation lambda.
    Secret {
        client: aws_sdk_secretsmanager::Client,
        secret_id: String,
    },
}

impl KeySource {
    pub fn fixed(key: impl Into<String>) -> Self {
        Self::Fixed(key.into())
    }

    pub fn secrets_manager(
        client: aws_sdk_secretsmanager::Client,
        secret_id: impl Into<String>,
    ) -> Self {
        Self::Secret {
            client,
            secret_id: secret_id.into(),
        }
    }

    /// Pick the key source from the environment, once at startup.
    ///
    /// 1. `JWT_KEY` — a literal key. Tests and offline work; wins if set.
    /// 2. `JWT_SECRET_ID` — the weekly-rotated Secrets Manager secret. Terraform
    ///    puts this on the deployed lambda, and `make run` reads it from
    ///    `.env.<stage>`. Building the AWS client is why this is `async`.
    /// 3. Neither — locally, fall back to a hardcoded key so `cargo run` works
    ///    with no AWS at all. In Lambda that would be a signing key published in
    ///    the repo, so it panics instead: a cold-start failure beats an API
    ///    anyone can mint tokens for.
    pub async fn from_env(in_lambda: bool) -> Self {
        if let Ok(key) = std::env::var("JWT_KEY") {
            if !key.is_empty() {
                tracing::info!("jwt key: JWT_KEY env var");
                return Self::fixed(key);
            }
        }

        if let Ok(secret_id) = std::env::var("JWT_SECRET_ID") {
            if !secret_id.is_empty() {
                tracing::info!("jwt key: secrets manager secret {secret_id}");
                let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
                return Self::secrets_manager(
                    aws_sdk_secretsmanager::Client::new(&config),
                    secret_id,
                );
            }
        }

        assert!(
            !in_lambda,
            "neither JWT_KEY nor JWT_SECRET_ID is set; terraform sets JWT_SECRET_ID on the lambda"
        );
        tracing::warn!("jwt key: INSECURE built-in development key (set JWT_KEY or JWT_SECRET_ID)");
        Self::fixed(DEV_FALLBACK_KEY)
    }
}

/// Builder for [`AuthState`]. Lets `app()` state the whole auth policy in one
/// place:
///
/// ```ignore
/// AuthConfig::new(keys)
///     .protect(vec!["/private"])
///     .allow_emails(vec!["dev@omoffice.example"])
///     .build()
/// ```
pub struct AuthConfig {
    source: KeySource,
    protected: Vec<String>,
    allowed_emails: Vec<String>,
}

impl AuthConfig {
    pub fn new(source: KeySource) -> Self {
        Self {
            source,
            protected: Vec::new(),
            allowed_emails: Vec::new(),
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

    /// Emails allowed to request a token. Matched case-insensitively.
    pub fn allow_emails<I, S>(mut self, emails: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_emails = emails
            .into_iter()
            .map(|email| email.into().trim().to_lowercase())
            .collect();
        self
    }

    pub fn build(self) -> AuthState {
        AuthState(Arc::new(Inner {
            source: self.source,
            protected: self.protected,
            allowed_emails: self.allowed_emails,
            cache: tokio::sync::RwLock::new(None),
        }))
    }
}

/// The signing key plus the one it replaced, if any.
#[derive(Clone)]
struct Keys {
    current: String,
    previous: Option<String>,
}

struct Cached {
    keys: Keys,
    fetched_at: Instant,
}

struct Inner {
    source: KeySource,
    protected: Vec<String>,
    allowed_emails: Vec<String>,
    cache: tokio::sync::RwLock<Option<Cached>>,
}

/// Shared auth state: the policy from [`AuthConfig`] plus the key cache. Cheap
/// to clone.
#[derive(Clone)]
pub struct AuthState(Arc<Inner>);

impl AuthState {
    /// True if `path` sits under one of the protected prefixes.
    fn is_protected(&self, path: &str) -> bool {
        self.0.protected.iter().any(|prefix| {
            path == prefix
                || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'))
        })
    }

    fn is_allowed(&self, email: &str) -> bool {
        self.0.allowed_emails.iter().any(|allowed| allowed == email)
    }

    async fn keys(&self) -> Result<Keys, AuthError> {
        let (client, secret_id) = match &self.0.source {
            KeySource::Fixed(key) => {
                return Ok(Keys {
                    current: key.clone(),
                    previous: None,
                })
            }
            KeySource::Secret { client, secret_id } => (client, secret_id),
        };

        if let Some(cached) = self.0.cache.read().await.as_ref() {
            if cached.fetched_at.elapsed() < KEY_CACHE_TTL {
                return Ok(cached.keys.clone());
            }
        }

        let current = fetch_stage(client, secret_id, "AWSCURRENT")
            .await?
            .ok_or_else(|| AuthError::KeyUnavailable(format!("{secret_id} has no AWSCURRENT")))?;
        // AWSPREVIOUS only exists after the first rotation.
        let previous = fetch_stage(client, secret_id, "AWSPREVIOUS").await?;

        let keys = Keys { current, previous };
        *self.0.cache.write().await = Some(Cached {
            keys: keys.clone(),
            fetched_at: Instant::now(),
        });
        Ok(keys)
    }

    /// Sign a 48h token for `email`. Returns the token and its expiry.
    pub async fn issue(&self, email: &str) -> Result<(String, u64), AuthError> {
        let now = unix_now();
        let claims = Claims {
            sub: email.to_string(),
            iat: now,
            exp: now + TOKEN_TTL.as_secs(),
        };
        let key = self.keys().await?.current;
        let token = encode(
            &Header::new(ALGORITHM),
            &claims,
            &EncodingKey::from_secret(key.as_bytes()),
        )
        .map_err(|e| AuthError::KeyUnavailable(e.to_string()))?;
        Ok((token, claims.exp))
    }

    /// Verify a token against the current key, then the previous one.
    pub async fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        let keys = self.keys().await?;
        let validation = validation();

        match decode_with(token, &keys.current, &validation) {
            Ok(claims) => return Ok(claims),
            // An expired token is expired under either key; report it as such.
            Err(AuthError::ExpiredToken) => return Err(AuthError::ExpiredToken),
            Err(_) => {}
        }

        match keys.previous {
            Some(previous) => decode_with(token, &previous, &validation),
            None => Err(AuthError::InvalidToken),
        }
    }
}

async fn fetch_stage(
    client: &aws_sdk_secretsmanager::Client,
    secret_id: &str,
    stage: &str,
) -> Result<Option<String>, AuthError> {
    let result = client
        .get_secret_value()
        .secret_id(secret_id)
        .version_stage(stage)
        .send()
        .await;

    match result {
        Ok(value) => Ok(value.secret_string().map(str::to_string)),
        // A missing stage is not an error: AWSPREVIOUS is absent until the first rotation.
        Err(err) if is_not_found(&err) => Ok(None),
        Err(err) => Err(AuthError::KeyUnavailable(format!(
            "{secret_id} ({stage}): {}",
            aws_sdk_secretsmanager::error::DisplayErrorContext(&err)
        ))),
    }
}

fn is_not_found(
    err: &aws_sdk_secretsmanager::error::SdkError<
        aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError,
    >,
) -> bool {
    matches!(err, aws_sdk_secretsmanager::error::SdkError::ServiceError(e) if e.err().is_resource_not_found_exception())
}

fn validation() -> Validation {
    let mut validation = Validation::new(ALGORITHM);
    validation.set_required_spec_claims(&["exp", "sub"]);
    validation
}

fn decode_with(token: &str, key: &str, validation: &Validation) -> Result<Claims, AuthError> {
    decode::<Claims>(token, &DecodingKey::from_secret(key.as_bytes()), validation)
        .map(|data| data.claims)
        .map_err(|err| match err.kind() {
            ErrorKind::ExpiredSignature => AuthError::ExpiredToken,
            _ => AuthError::InvalidToken,
        })
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_secs()
}

/// `GET /auth/request-key/{email}` — issue a token to a whitelisted email.
///
/// This is also where a person first becomes a row in `users`: the email on the
/// token is the key every other table hangs off.
pub async fn request_key(
    State(state): State<AuthState>,
    Extension(db): Extension<Db>,
    Path(email): Path<String>,
) -> Result<Json<Value>, AuthError> {
    let email = email.trim().to_lowercase();
    if !state.is_allowed(&email) {
        tracing::warn!("key requested for non-whitelisted email");
        return Err(AuthError::EmailNotAllowed);
    }

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

/// Router-wide middleware. Requests outside the protected prefixes pass straight
/// through; protected ones need a valid bearer token, whose [`Claims`] are then
/// available to the handler as an extension.
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
        iat: unix_now(),
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
            .allow_emails(vec!["Dev@Omoffice.Example"])
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

    #[test]
    fn the_whitelist_ignores_case() {
        let state = state();
        assert!(state.is_allowed("dev@omoffice.example"));
        assert!(!state.is_allowed("stranger@example.com"));
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
