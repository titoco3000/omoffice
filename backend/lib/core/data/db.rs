//! Aurora DSQL access, authenticated with a SigV4 IAM token rather than a
//! stored password: [`Inner::refresh_token_if_stale`] re-signs one before it
//! expires, so the pool never needs a password on disk. DSQL also has no row
//! locks -- it is serializable instead, so a racing pair of writes fails
//! with a conflict rather than corrupting each other; [`retry_on_conflict`]
//! is how a write survives that.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aws_config::SdkConfig;
use aws_sdk_dsql::auth_token::{AuthTokenGenerator, Config as AuthTokenConfig};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};

const TOKEN_TTL: Duration = Duration::from_secs(900);
const TOKEN_REFRESH_AFTER: Duration = Duration::from_secs(600);
const CONNECTION_MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);
const MAX_CONNECTIONS: u32 = 5;
const DEFAULT_DATABASE: &str = "postgres";
const DEFAULT_USER: &str = "admin";
const SQLSTATE_SERIALIZATION_FAILURE: &str = "40001";
const MAX_WRITE_ATTEMPTS: u32 = 4;

/// Handle to the stage's DSQL cluster. `None` means no `DATABASE_ENDPOINT`
/// was configured, so every route reports `database_unavailable`. Cheap to
/// clone -- an `Arc` inside -- so it travels as an axum `Extension`.
#[derive(Clone)]
pub struct Db(Option<Arc<Inner>>);

struct Inner {
    pool: PgPool,
    generator: AuthTokenGenerator,
    sdk_config: SdkConfig,
    host: String,
    user: String,
    database: String,
    refresh_at: tokio::sync::RwLock<Instant>,
}

impl Db {
    /// No cluster behind it: every query reports `database_unavailable`.
    #[cfg(test)]
    pub fn disabled() -> Self {
        Self(None)
    }

    /// Build the pool from `DATABASE_ENDPOINT`. Lazy: nothing talks to AWS
    /// or the cluster here, so a cold start pays neither cost.
    pub async fn from_env() -> Self {
        let Some(host) = std::env::var("DATABASE_ENDPOINT")
            .ok()
            .filter(|host| !host.is_empty())
        else {
            tracing::warn!(
                "no DATABASE_ENDPOINT: database routes will report database_unavailable"
            );
            return Self(None);
        };

        let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let Some(region) = sdk_config.region().cloned() else {
            tracing::error!("no AWS region resolved; cannot sign DSQL auth tokens");
            return Self(None);
        };

        let auth_config = AuthTokenConfig::builder()
            .hostname(&host)
            .region(region)
            .expires_in(TOKEN_TTL.as_secs())
            .build();
        let auth_config = match auth_config {
            Ok(config) => config,
            Err(err) => {
                tracing::error!("could not configure the DSQL token generator: {err}");
                return Self(None);
            }
        };

        let user = env_or("DATABASE_USER", DEFAULT_USER);
        let database = env_or("DATABASE_NAME", DEFAULT_DATABASE);
        tracing::info!("database: {user}@{host}/{database}");

        // The password is filled in by the first refresh; `refresh_at` is
        // already in the past, so that happens before the first connection.
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .max_lifetime(CONNECTION_MAX_LIFETIME)
            .connect_lazy_with(connect_options(&host, &user, &database, String::new()));

        Self(Some(Arc::new(Inner {
            pool,
            generator: AuthTokenGenerator::new(auth_config),
            sdk_config,
            host,
            user,
            database,
            refresh_at: tokio::sync::RwLock::new(Instant::now()),
        })))
    }

    /// The pool, with a token that is still valid.
    pub async fn pool(&self) -> Result<&PgPool, DbError> {
        let inner = self.0.as_ref().ok_or(DbError::Unavailable)?;
        inner.refresh_token_if_stale().await?;
        Ok(&inner.pool)
    }
}

impl Inner {
    async fn refresh_token_if_stale(&self) -> Result<(), DbError> {
        if Instant::now() < *self.refresh_at.read().await {
            return Ok(());
        }
        let mut refresh_at = self.refresh_at.write().await;
        if Instant::now() < *refresh_at {
            return Ok(());
        }

        let token = self
            .generator
            .db_connect_admin_auth_token(&self.sdk_config)
            .await
            .map_err(|err| DbError::Token(err.to_string()))?;

        self.pool.set_connect_options(connect_options(
            &self.host,
            &self.user,
            &self.database,
            token.as_str().to_string(),
        ));
        *refresh_at = Instant::now() + TOKEN_REFRESH_AFTER;
        tracing::debug!("signed a fresh DSQL auth token");
        Ok(())
    }
}

fn connect_options(host: &str, user: &str, database: &str, token: String) -> PgConnectOptions {
    PgConnectOptions::new()
        .host(host)
        .username(user)
        .database(database)
        .password(&token)
        .ssl_mode(PgSslMode::VerifyFull)
        .application_name("omoffice-api")
}

fn env_or(name: &str, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// Run a write, retrying while DSQL reports a commit-time conflict (no
/// locks: the closure re-runs from the top, so it must be safe to repeat).
pub async fn retry_on_conflict<F, Fut, T>(mut operation: F) -> Result<T, DbError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, sqlx::Error>>,
{
    let mut backoff = Duration::from_millis(20);
    for attempt in 1..=MAX_WRITE_ATTEMPTS {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) if is_conflict(&err) && attempt < MAX_WRITE_ATTEMPTS => {
                tracing::warn!("dsql write conflict (attempt {attempt}), retrying: {err}");
                tokio::time::sleep(backoff).await;
                backoff *= 2;
            }
            Err(err) => return Err(DbError::Query(err)),
        }
    }
    unreachable!("the loop returns on the last attempt")
}

fn is_conflict(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db)
        if db.code().as_deref() == Some(SQLSTATE_SERIALIZATION_FAILURE))
}

/// What the database layer can fail with.
#[derive(Debug)]
pub enum DbError {
    /// No `DATABASE_ENDPOINT` configured.
    Unavailable,
    /// The IAM auth token could not be signed: no credentials, or no permission.
    Token(String),
    /// Anything the cluster itself rejected.
    Query(sqlx::Error),
}

impl From<sqlx::Error> for DbError {
    fn from(err: sqlx::Error) -> Self {
        Self::Query(err)
    }
}

impl IntoResponse for DbError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "this API instance has no DATABASE_ENDPOINT configured".to_string(),
            ),
            Self::Token(why) => {
                tracing::error!("dsql auth token failed: {why}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_auth_failed",
                    format!("could not sign a DSQL auth token: {why}"),
                )
            }
            Self::Query(err) => {
                tracing::error!("dsql query failed: {err}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    format!("the database rejected the query: {err}"),
                )
            }
        };
        (
            status,
            Json(serde_json::json!({ "error": error, "message": message })),
        )
            .into_response()
    }
}
