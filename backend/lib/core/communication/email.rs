//! Email -- a stub. Prints what it would send; swap the body for a real
//! SES/SMTP call and nothing calling it needs to change.

pub fn send_email(from: &str, to: &str, title: &str, content: &str) {
    println!("[email stub] from={from} to={to} title={title:?}\n{content}");
}
