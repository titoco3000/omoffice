//! Live browser sockets held by *this* process -- the local-dev half of
//! delivery. Keyed by connection id, the same id `connections` stores, so
//! [`super::Messenger`] resolves a roster once (from the database) and
//! delivers to whichever half actually holds the socket without a second
//! lookup here. Knows nothing about channels, chat or invites.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};

/// The outbound half of one registered socket; [`Hub::bind`] owns the
/// receiving half.
type Outbox = mpsc::UnboundedSender<Value>;

/// Registry of open sockets, keyed by connection id. Cheap to clone -- an
/// `Arc` inside, same as [`crate::core::data::db::Db`].
#[derive(Clone, Default)]
pub struct Hub {
    sockets: Arc<RwLock<HashMap<String, Outbox>>>,
}

impl Hub {
    async fn register(&self, connection_id: &str) -> mpsc::UnboundedReceiver<Value> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.sockets
            .write()
            .await
            .insert(connection_id.to_string(), tx);
        rx
    }

    async fn unregister(&self, connection_id: &str) {
        self.sockets.write().await.remove(connection_id);
    }

    /// Push `frame` down one socket, if this process holds it. Returns
    /// whether it does -- that is also how [`super::Messenger`] tells a
    /// local connection apart from an `apigw` one without a stored label:
    /// not registered here means it must be reached through the Management
    /// API instead.
    pub async fn send(&self, connection_id: &str, frame: &Value) -> bool {
        let Some(tx) = self.sockets.read().await.get(connection_id).cloned() else {
            return false;
        };
        tx.send(frame.clone()).is_ok()
    }

    /// Drive one accepted socket end to end until it closes, then
    /// unregisters itself -- callers never call [`unregister`](Self::unregister)
    /// directly. `on_message` runs once per JSON frame the client sends.
    pub async fn bind<F, Fut>(&self, socket: WebSocket, connection_id: String, on_message: F)
    where
        F: Fn(Value) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        let mut rx = self.register(&connection_id).await;
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
        self.unregister(&connection_id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `bind` needs a real `WebSocket`, so these exercise the registry directly.
    #[tokio::test]
    async fn a_registered_socket_receives_what_is_sent_to_its_connection_id() {
        let hub = Hub::default();
        let mut rx = hub.register("conn-1").await;

        hub.send("conn-1", &json!({ "text": "hi" })).await;

        assert_eq!(rx.recv().await, Some(json!({ "text": "hi" })));
    }

    #[tokio::test]
    async fn sending_to_an_unregistered_connection_is_a_no_op() {
        let hub = Hub::default();
        // Would panic on a missing key if this indexed instead of `get`-ing.
        hub.send("conn-nobody", &json!({ "text": "hi" })).await;
    }

    #[tokio::test]
    async fn a_second_tab_is_its_own_connection() {
        let hub = Hub::default();
        let mut rx_a = hub.register("conn-a").await;
        let mut rx_b = hub.register("conn-b").await;

        hub.send("conn-a", &json!({ "text": "hi" })).await;

        assert_eq!(rx_a.recv().await, Some(json!({ "text": "hi" })));
        assert!(rx_b.try_recv().is_err());
    }

    #[tokio::test]
    async fn unregistering_stops_delivery_without_disturbing_another_tab() {
        let hub = Hub::default();
        let mut rx_a = hub.register("conn-a").await;
        let mut rx_b = hub.register("conn-b").await;

        hub.unregister("conn-a").await;
        hub.send("conn-a", &json!({ "text": "hi" })).await;
        hub.send("conn-b", &json!({ "text": "hi" })).await;

        // The channel is dropped along with the registration, so the sender
        // half is gone too and `recv` reports the channel closed.
        assert_eq!(rx_a.recv().await, None);
        assert_eq!(rx_b.recv().await, Some(json!({ "text": "hi" })));
    }
}
