//! Live browser sessions: who has a socket open, and how to push a JSON
//! value to them. Knows nothing about chat or any other feature.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};

/// The outbound half of one registered socket; [`Hub::bind`] owns the receiving half.
type Outbox = mpsc::UnboundedSender<Value>;

/// One registration within a single email's connections, so closing one tab
/// doesn't disturb another open at the same time.
type ConnId = u64;

#[derive(Default)]
struct State {
    by_email: HashMap<String, HashMap<ConnId, Outbox>>,
}

/// Registry of open sockets, keyed by email. Cheap to clone (an `Arc`
/// inside), same as `Db`.
#[derive(Clone, Default)]
pub struct Hub {
    state: Arc<RwLock<State>>,
    next_id: Arc<AtomicU64>,
}

impl Hub {
    async fn register(&self, email: &str) -> (ConnId, mpsc::UnboundedReceiver<Value>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::unbounded_channel();
        self.state
            .write()
            .await
            .by_email
            .entry(email.to_string())
            .or_default()
            .insert(id, tx);
        (id, rx)
    }

    async fn unregister(&self, email: &str, id: ConnId) {
        let mut state = self.state.write().await;
        if let Some(conns) = state.by_email.get_mut(email) {
            conns.remove(&id);
            if conns.is_empty() {
                state.by_email.remove(email);
            }
        }
    }

    /// Push `body` to every socket `to` has open. A dead send is dropped
    /// silently — nobody is listening, so nobody can be told.
    pub async fn send(&self, to: &str, body: &Value) {
        if let Some(conns) = self.state.read().await.by_email.get(to) {
            for tx in conns.values() {
                let _ = tx.send(body.clone());
            }
        }
    }

    /// [`send`](Self::send) to several recipients at once — chat's fan-out
    /// to a roster.
    pub async fn send_to(&self, recipients: &[String], body: &Value) {
        for email in recipients {
            self.send(email, body).await;
        }
    }

    /// Drive one accepted socket end to end until it closes, then
    /// unregisters itself — callers never call [`unregister`](Self::unregister).
    pub async fn bind<F, Fut>(&self, socket: WebSocket, email: String, on_message: F)
    where
        F: Fn(Value) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        let (id, mut rx) = self.register(&email).await;
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
        self.unregister(&email, id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `bind` needs a real `WebSocket`, so these exercise the registry directly.
    #[tokio::test]
    async fn a_registered_connection_receives_what_is_sent_to_its_email() {
        let hub = Hub::default();
        let (_id, mut rx) = hub.register("dev@omoffice.example").await;

        hub.send("dev@omoffice.example", &json!({ "text": "hi" }))
            .await;

        assert_eq!(rx.recv().await, Some(json!({ "text": "hi" })));
    }

    #[tokio::test]
    async fn sending_to_an_email_with_nothing_registered_is_a_no_op() {
        let hub = Hub::default();
        // Would panic on a missing key if this indexed instead of `get`-ing.
        hub.send("nobody@example.com", &json!({ "text": "hi" }))
            .await;
    }

    #[tokio::test]
    async fn a_second_tab_for_the_same_email_gets_its_own_registration() {
        let hub = Hub::default();
        let (_id_a, mut rx_a) = hub.register("dev@omoffice.example").await;
        let (_id_b, mut rx_b) = hub.register("dev@omoffice.example").await;

        hub.send("dev@omoffice.example", &json!({ "text": "hi" }))
            .await;

        assert_eq!(rx_a.recv().await, Some(json!({ "text": "hi" })));
        assert_eq!(rx_b.recv().await, Some(json!({ "text": "hi" })));
    }

    #[tokio::test]
    async fn unregistering_stops_delivery_without_disturbing_another_tab() {
        let hub = Hub::default();
        let (id_a, mut rx_a) = hub.register("dev@omoffice.example").await;
        let (_id_b, mut rx_b) = hub.register("dev@omoffice.example").await;

        hub.unregister("dev@omoffice.example", id_a).await;
        hub.send("dev@omoffice.example", &json!({ "text": "hi" }))
            .await;

        // The channel is dropped along with the registration, so the sender
        // half is gone too and `recv` reports the channel closed.
        assert_eq!(rx_a.recv().await, None);
        assert_eq!(rx_b.recv().await, Some(json!({ "text": "hi" })));
    }

    #[tokio::test]
    async fn send_to_reaches_every_listed_recipient_and_skips_the_rest() {
        let hub = Hub::default();
        let (_id_a, mut rx_a) = hub.register("a@example.com").await;
        let (_id_b, mut rx_b) = hub.register("b@example.com").await;
        let (_id_c, mut rx_c) = hub.register("c@example.com").await;

        hub.send_to(
            &["a@example.com".to_string(), "b@example.com".to_string()],
            &json!({ "text": "hi" }),
        )
        .await;

        assert_eq!(rx_a.recv().await, Some(json!({ "text": "hi" })));
        assert_eq!(rx_b.recv().await, Some(json!({ "text": "hi" })));
        assert!(rx_c.try_recv().is_err());
    }
}
