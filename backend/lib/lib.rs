//! Library root: the whole app as a set of modules, exercisable headless
//! (`cargo test --lib`) without a listening socket or a binary wrapped
//! around it.

pub mod core;
pub mod features;
pub mod router;
pub mod schema;
