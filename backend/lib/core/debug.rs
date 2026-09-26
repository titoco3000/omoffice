use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;

fn enabled() -> bool {
    let value = std::env::var("DEBUG").unwrap_or_default();
    !matches!(value.to_ascii_lowercase().as_str(), "" | "0" | "false")
}

pub async fn log_requests(request: Request, next: Next) -> Response {
    if !enabled() {
        return next.run(request).await;
    }

    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let params = request.uri().query().unwrap_or("").to_string();
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap_or_default();
    let body_text = String::from_utf8_lossy(&bytes);

    if method != Method::OPTIONS {
        tracing::info!("{method} {path} {params} {body_text}");
    }

    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

/// Step-by-step timing for a slow multi-step operation: each [`lap`](Self::lap)
/// logs how long that step took and the running total. Meant for chasing
/// latency, not left in hot paths for good.
pub struct Laps {
    label: String,
    started: std::time::Instant,
    last: std::time::Instant,
}

impl Laps {
    pub fn new(label: impl Into<String>) -> Self {
        let now = std::time::Instant::now();
        Self {
            label: label.into(),
            started: now,
            last: now,
        }
    }

    pub fn lap(&mut self, step: &str) {
        let now = std::time::Instant::now();
        tracing::info!(
            "timing {}: {step} took {:?} (total {:?})",
            self.label,
            now - self.last,
            now - self.started
        );
        self.last = now;
    }
}
