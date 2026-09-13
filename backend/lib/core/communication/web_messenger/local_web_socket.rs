//! The local-dev transport, and (see the module doc on [`super`]) the only
//! websocket code this build ever compiles: a plain in-process hashtable,
//! registered under a fresh connection id (one user may have several tabs
//! open, each its own connection) but addressed by user id, so
//! [`Hub::send`] fans out to every tab at once without exposing a
//! connection id above this module. Knows nothing about channels, chat or
//! invites -- only that sockets exist and belong to users.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Query};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use super::{Channel, Messenger, WsAuth, CONNECTED, DISCONNECTED};
use crate::core::auth;

type Outbox = mpsc::UnboundedSender<Value>;

#[derive(Default)]
struct Sockets {
    /// One entry per open tab.
    by_connection: HashMap<String, Outbox>,
    /// Which connections belong to which user, so `send` need not scan
    /// every open socket to find one user's tabs.
    by_user: HashMap<String, Vec<String>>,
}

/// Registry of open sockets. Cheap to clone -- an `Arc` inside.
#[derive(Clone, Default)]
pub struct Hub {
    sockets: Arc<RwLock<Sockets>>,
}

impl Hub {
    async fn register(&self, user_id: &str) -> (String, mpsc::UnboundedReceiver<Value>) {
        let connection_id = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        let mut sockets = self.sockets.write().await;
        sockets.by_connection.insert(connection_id.clone(), tx);
        sockets
            .by_user
            .entry(user_id.to_string())
            .or_default()
            .push(connection_id.clone());
        (connection_id, rx)
    }

    async fn unregister(&self, user_id: &str, connection_id: &str) {
        let mut sockets = self.sockets.write().await;
        sockets.by_connection.remove(connection_id);
        if let Some(connections) = sockets.by_user.get_mut(user_id) {
            connections.retain(|id| id != connection_id);
            if connections.is_empty() {
                sockets.by_user.remove(user_id);
            }
        }
    }

    /// Push `frame` to every tab `user_id` has open. Nobody registered under
    /// that id is a no-op, not an error -- nobody is listening, so nobody
    /// can be told.
    async fn send(&self, user_id: &str, frame: &Value) {
        let sockets = self.sockets.read().await;
        let Some(connections) = sockets.by_user.get(user_id) else {
            return;
        };
        for connection_id in connections {
            if let Some(tx) = sockets.by_connection.get(connection_id) {
                let _ = tx.send(frame.clone());
            }
        }
    }

    async fn is_connected(&self, user_id: &str) -> bool {
        self.sockets
            .read()
            .await
            .by_user
            .get(user_id)
            .is_some_and(|connections| !connections.is_empty())
    }

    /// Drive one accepted socket end to end until it closes, then
    /// unregisters itself -- callers never call [`unregister`](Self::unregister)
    /// directly. `on_message` runs once per JSON frame the client sends,
    /// already knowing `user_id`: there is no separate lookup to do to
    /// answer "who sent this", unlike the deployed transport.
    async fn bind<F, Fut>(&self, socket: WebSocket, user_id: String, on_message: F)
    where
        F: Fn(Value) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        let (connection_id, mut rx) = self.register(&user_id).await;
        let (mut sink, mut stream) = socket.split();

        let outbound = tokio::spawn(async move {
            while let Some(body) = rx.recv().await {
                let Ok(text) = serde_json::to_string(&body) else {
                    continue;
                };
                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        });

        while let Some(Ok(frame)) = stream.next().await {
            if let Message::Text(text) = frame {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    on_message(value).await;
                }
            }
        }

        outbound.abort();
        self.unregister(&user_id, &connection_id).await;
    }
}

impl Messenger {
    /// This build's only transport is a hashtable in this process, so
    /// `_db` (kept for a uniform constructor signature between the two
    /// transports) never touches a database at all.
    pub async fn new(_db: crate::core::data::db::Db) -> Self {
        Self {
            hub: Hub::default(),
            handlers: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Send `message` on `channel` to each of `to` -- every local tab this
    /// process itself holds. Best effort: a recipient with no open socket
    /// simply misses it.
    pub async fn send(&self, channel: Channel, to: &[String], message: Value) {
        let frame = serde_json::json!({ "channel": channel, "message": message });
        for user_id in to {
            self.hub.send(user_id, &frame).await;
        }
    }

    /// True if `user_id` has at least one open socket right now.
    pub async fn is_connected(&self, user_id: &str) -> bool {
        self.hub.is_connected(user_id).await
    }
}

/// `GET /ws?token=...` -- one per browser tab, carrying every channel; this
/// process holds it open itself, and forgets it the moment it closes. No
/// database involved at any point -- see the module doc on [`super`].
pub async fn upgrade_route(
    Extension(messenger): Extension<Messenger>,
    Query(params): Query<WsAuth>,
    ws: WebSocketUpgrade,
) -> Response {
    let claims = match auth::verify(&params.token).await {
        Ok(claims) => claims,
        Err(err) => return err.into_response(),
    };

    ws.on_upgrade(move |socket| async move {
        let user_id = claims.sub;
        messenger.notify_connection(&user_id, CONNECTED).await;

        let inbound = messenger.clone();
        let dispatch_id = user_id.clone();
        messenger
            .hub
            .bind(socket, user_id.clone(), move |frame| {
                let messenger = inbound.clone();
                let user_id = dispatch_id.clone();
                async move { messenger.dispatch(&user_id, frame).await }
            })
            .await;

        messenger.notify_connection(&user_id, DISCONNECTED).await;
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `bind` needs a real `WebSocket`, so these exercise the registry directly.
    #[tokio::test]
    async fn a_registered_socket_receives_what_is_sent_to_its_user() {
        let hub = Hub::default();
        let (_id, mut rx) = hub.register("user-1").await;

        hub.send("user-1", &json!({ "text": "hi" })).await;

        assert_eq!(rx.recv().await, Some(json!({ "text": "hi" })));
    }

    #[tokio::test]
    async fn sending_to_an_unregistered_user_is_a_no_op() {
        let hub = Hub::default();
        // Would panic on a missing key if this indexed instead of `get`-ing.
        hub.send("nobody", &json!({ "text": "hi" })).await;
    }

    #[tokio::test]
    async fn a_second_tab_is_its_own_connection_but_the_same_user() {
        let hub = Hub::default();
        let (_id_a, mut rx_a) = hub.register("user-1").await;
        let (_id_b, mut rx_b) = hub.register("user-1").await;

        hub.send("user-1", &json!({ "text": "hi" })).await;

        assert_eq!(rx_a.recv().await, Some(json!({ "text": "hi" })));
        assert_eq!(rx_b.recv().await, Some(json!({ "text": "hi" })));
    }

    #[tokio::test]
    async fn unregistering_one_tab_does_not_disturb_another_users_tab() {
        let hub = Hub::default();
        let (id_a, mut rx_a) = hub.register("user-a").await;
        let (_id_b, mut rx_b) = hub.register("user-b").await;

        hub.unregister("user-a", &id_a).await;
        hub.send("user-a", &json!({ "text": "hi" })).await;
        hub.send("user-b", &json!({ "text": "hi" })).await;

        // The channel is dropped along with the registration, so the sender
        // half is gone too and `recv` reports the channel closed.
        assert_eq!(rx_a.recv().await, None);
        assert_eq!(rx_b.recv().await, Some(json!({ "text": "hi" })));
    }

    #[tokio::test]
    async fn is_connected_reflects_registration_and_unregistration() {
        let hub = Hub::default();
        assert!(!hub.is_connected("user-1").await);

        let (id, _rx) = hub.register("user-1").await;
        assert!(hub.is_connected("user-1").await);

        hub.unregister("user-1", &id).await;
        assert!(!hub.is_connected("user-1").await);
    }
}
