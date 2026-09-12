//! Aurora DSQL access, authenticated with a SigV4 IAM token. See CLAUDE.md
//! for the token-refresh and no-`sqlx::query!` rationale.

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

/// How long a generated IAM token stays valid.
const TOKEN_TTL: Duration = Duration::from_secs(900);

/// How long one is reused before being re-signed. Comfortably inside
/// [`TOKEN_TTL`], so a connection opened just after the check still authenticates.
const TOKEN_REFRESH_AFTER: Duration = Duration::from_secs(600);

/// DSQL closes a connection after an hour; recycle well before that.
const CONNECTION_MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);

/// A lambda serves one request at a time, so this is really the local-run cap.
const MAX_CONNECTIONS: u32 = 5;

/// The single database every DSQL cluster has, and the built-in role our IAM
/// policy grants (`dsql:DbConnectAdmin`).
const DEFAULT_DATABASE: &str = "postgres";
const DEFAULT_USER: &str = "admin";

/// SQLSTATE for a serialization failure. DSQL's optimistic concurrency control
/// reports a commit-time conflict this way (`OC000` data, `OC001` schema).
const SQLSTATE_SERIALIZATION_FAILURE: &str = "40001";

/// How many times a conflicting write is retried before giving up.
const MAX_WRITE_ATTEMPTS: u32 = 4;

/// Handle to the stage's DSQL cluster. `None` means no `DATABASE_ENDPOINT`
/// was configured, so every route reports `database_unavailable`.
#[derive(Clone)]
pub struct Db(Option<Arc<Inner>>);

struct Inner {
    pool: PgPool,
    generator: AuthTokenGenerator,
    sdk_config: SdkConfig,
    host: String,
    user: String,
    database: String,
    /// When the pool's token should be replaced. Guards the re-sign so
    /// concurrent callers do not all do it.
    refresh_at: tokio::sync::RwLock<Instant>,
}

impl Db {
    /// No cluster behind it: every query reports `database_unavailable`. Tests
    /// only — it is also what `from_env` returns when nothing is configured.
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
        // Another caller may have re-signed while we waited for the lock.
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

/// DSQL rejects anything but TLS, and the endpoint carries a publicly-signed
/// certificate, so the strictest mode is also the one that just works.
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
    /// No `DATABASE_ENDPOINT` — tests, or a run that was never handed the
    /// values from `infra/make output <stage>`.
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
