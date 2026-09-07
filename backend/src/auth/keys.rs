//! Where the HMAC signing key comes from, and [`AuthState`]: the policy plus
//! the key cache. The key rotates weekly (infra rotation lambda); tokens are
//! checked against both the current and previous key so one survives a
//! rotation after it's issued.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{
    decode, encode, errors::ErrorKind, DecodingKey, EncodingKey, Header, Validation,
};

use super::{AuthError, Claims, ALGORITHM, TOKEN_TTL};

/// How long a key fetched from Secrets Manager is reused before refetching.
/// Rotation is weekly, so a few minutes of staleness is harmless.
const KEY_CACHE_TTL: Duration = Duration::from_secs(300);

/// Key used when nothing is configured. Local development only —
/// [`KeySource::from_env`] refuses to hand it back inside Lambda.
const DEV_FALLBACK_KEY: &str = "insecure-local-development-key";

/// Where the HMAC signing key comes from.
pub enum KeySource {
    /// `JWT_KEY`, the dev fallback, or a test key. Nothing to fetch.
    Fixed(String),
    /// Secrets Manager. Fetched and cached by [`AuthState::keys`].
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

    /// Pick the key source once at startup: `JWT_KEY`, then `JWT_SECRET_ID`,
    /// then the dev fallback key. `in_lambda` turns that fallback into a
    /// hard startup error — never run Lambda on the insecure key.
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

/// Signing key plus the one it replaced. `previous` is `None` before the
/// first rotation.
#[derive(Clone)]
struct Keys {
    current: String,
    previous: Option<String>,
}

/// A [`Keys`] fetch, timestamped so `keys()` knows when to refetch.
struct Cached {
    keys: Keys,
    fetched_at: Instant,
}

struct Inner {
    source: KeySource,
    protected: Vec<String>,
    cache: tokio::sync::RwLock<Option<Cached>>,
}

/// Shared auth state: the policy from [`super::AuthConfig`] plus the key
/// cache. Cheap to clone.
#[derive(Clone)]
pub struct AuthState(Arc<Inner>);

/// Builds an [`AuthState`] from [`super::AuthConfig`]'s parts.
pub(super) fn build(source: KeySource, protected: Vec<String>) -> AuthState {
    AuthState(Arc::new(Inner {
        source,
        protected,
        cache: tokio::sync::RwLock::new(None),
    }))
}

impl AuthState {
    /// True if `path` sits under one of the protected prefixes.
    pub(super) fn is_protected(&self, path: &str) -> bool {
        self.0.protected.iter().any(|prefix| {
            path == prefix
                || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'))
        })
    }

    /// Cached key if fresh, else fetch and recache. `Fixed` has nothing to
    /// cache, always returns the same key.
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
        // AWSPREVIOUS: absent before first rotation, not an error.
        let previous = fetch_stage(client, secret_id, "AWSPREVIOUS").await?;

        let keys = Keys { current, previous };
        *self.0.cache.write().await = Some(Cached {
            keys: keys.clone(),
            fetched_at: Instant::now(),
        });
        Ok(keys)
    }

    /// Sign a token for `email`. Returns the token and its expiry.
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

    /// Verify against `current`, retry `previous` on a bad signature so a
    /// token issued just before rotation still works. Don't retry on
    /// expiry — same expiry either key.
    pub async fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        let keys = self.keys().await?;
        let validation = validation();

        match decode_with(token, &keys.current, &validation) {
            Ok(claims) => return Ok(claims),
            Err(AuthError::ExpiredToken) => return Err(AuthError::ExpiredToken),
            Err(_) => {}
        }

        match keys.previous {
            Some(previous) => decode_with(token, &previous, &validation),
            None => Err(AuthError::InvalidToken),
        }
    }
}

/// Fetch one version stage (`"AWSCURRENT"`/`"AWSPREVIOUS"`) of the secret.
/// `Ok(None)` = stage not found, everything else is a real error.
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
        Err(err) if is_stage_not_found(&err) => Ok(None),
        Err(err) => Err(AuthError::KeyUnavailable(format!(
            "{secret_id} ({stage}): {}",
            aws_sdk_secretsmanager::error::DisplayErrorContext(&err)
        ))),
    }
}

fn is_stage_not_found(
    err: &aws_sdk_secretsmanager::error::SdkError<
        aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError,
    >,
) -> bool {
    matches!(err, aws_sdk_secretsmanager::error::SdkError::ServiceError(e) if e.err().is_resource_not_found_exception())
}

/// jsonwebtoken doesn't require `exp`/`sub` by default; require them.
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

pub(super) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_secs()
}
