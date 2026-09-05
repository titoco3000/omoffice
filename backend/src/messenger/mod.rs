//! Outbound messages to a user, one submodule per delivery channel.
//!
//! [`websocket`] pushes to whatever browser sessions a user currently has
//! open; [`email`] is a stub that prints to the console instead of actually
//! sending anything. Adding a channel later (SMS, a push notification) is a
//! new submodule with its own `send(&self, to: &str, body: &Value)` plus one
//! field and one call in [`Messenger::notify`] below — nothing that already
//! calls `notify` has to change.
//!
//! This is deliberately not a trait/registry: there are two channels today
//! and `Messenger` is the one place that already knows both of them, the
//! same flat style `core::chime`/`core::db` use for their own single job.

pub mod email;
pub mod websocket;

use serde_json::Value;

/// Every configured channel. Reaches handlers as an `Extension`, the same
/// way `Db`/`ChimeState` do. Cheap to clone: `websocket::Hub` is an `Arc`
/// around its registry, `email::EmailChannel` carries no state at all.
#[derive(Clone, Default)]
pub struct Messenger {
    websocket: websocket::Hub,
    // Only read from `notify`, below. No route calls `notify` yet -- `chat`
    // talks to `websocket` directly for its room broadcast -- so this is
    // dead code by the binary's own accounting until a feature needs
    // one-recipient delivery (a mention, an invite email).
    #[allow(dead_code)]
    email: email::EmailChannel,
}

impl Messenger {
    pub fn new() -> Self {
        Self::default()
    }

    /// The live websocket hub, for a feature (chat) that needs its own
    /// registration/broadcast logic rather than one-at-a-time delivery
    /// through [`notify`] — `notify` is for "tell this one user something",
    /// not "run a room".
    pub fn websocket(&self) -> &websocket::Hub {
        &self.websocket
    }

    /// Deliver `body` to `to` on every channel. Best-effort on all of them: a
    /// channel with nobody listening (no socket open) or nothing configured
    /// (no real email backend yet) just doesn't deliver, silently — same as
    /// `auth::request_key`'s best-effort write to `users`.
    ///
    /// Unused so far: `chat` is the only feature built on `Messenger`, and it
    /// broadcasts to a roster via [`websocket`](Self::websocket) rather than
    /// notifying one recipient at a time. Kept as the intended entry point
    /// for a feature that does (a mention, an invite email).
    #[allow(dead_code)]
    pub async fn notify(&self, to: &str, body: &Value) {
        self.websocket.send(to, body).await;
        self.email.send(to, body).await;
    }
}
