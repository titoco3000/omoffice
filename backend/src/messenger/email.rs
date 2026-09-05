//! Email channel — a stub. No SMTP, no SES, nothing configured: it just
//! prints what it would have sent, so a caller can be written against the
//! real shape (`send(&self, to, body)`) before there is a real backend to
//! call. Swap the body of `send` for an SES/SMTP call when there is one;
//! nothing calling [`crate::messenger::Messenger::notify`] needs to change.

use serde_json::Value;

#[derive(Clone, Copy, Default)]
pub struct EmailChannel;

impl EmailChannel {
    // Only called from `Messenger::notify`, itself currently unused -- see
    // that method's doc comment.
    #[allow(dead_code)]
    pub async fn send(&self, to: &str, body: &Value) {
        println!("[email stub] to={to}: {body}");
    }
}
