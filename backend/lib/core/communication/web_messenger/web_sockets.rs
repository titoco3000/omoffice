//! Live browser sockets held by this process. Registered under a fresh
//! connection id (one user may have several tabs open, each its own
//! connection) but addressed by user id, so [`Hub::send`] fans out to every
//! tab at once without exposing a connection id at all. Knows nothing about
//! channels, chat or invites -- only that sockets exist and belong to users.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

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
    pub async fn send(&self, user_id: &str, frame: &Value) {
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

    pub async fn is_connected(&self, user_id: &str) -> bool {
        self.sockets
            .read()
            .await
            .by_user
            .get(user_id)
            .is_some_and(|connections| !connections.is_empty())
    }

    /// Drive one accepted socket end to end until it closes, then
    /// unregisters itself -- callers never call [`unregister`](Self::unregister)
    /// directly. `on_message` runs once per JSON frame the client sends.
    pub async fn bind<F, Fut>(&self, socket: WebSocket, user_id: String, on_message: F)
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
