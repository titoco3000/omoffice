//! Email channel — a stub. Prints what it would send; swap `send`'s body
//! for a real SES/SMTP call and nothing calling it needs to change.

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
