//! A downstream facade with a renamed dependency and re-exported macros.

pub use emerald::prelude;
#[cfg(feature = "macros")]
pub use emerald::{command, commands};
#[cfg(feature = "async")]
pub use emerald::tokio;
