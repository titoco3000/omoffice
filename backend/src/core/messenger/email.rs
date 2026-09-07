//! Email channel — a stub. Prints what it would send; swap `send`'s body
//! for a real SES/SMTP call and nothing calling it needs to change.

use serde_json::Value;

#[derive(Clone, Copy, Default)]
pub struct EmailChannel;

impl EmailChannel {
    pub async fn send(&self, to: &str, channel: &str, message: &Value) {
        println!("[email stub] to={to} channel={channel}: {message}");
    }
}
