//! Notification service implementations, each behind its own Cargo feature.

#[cfg(feature = "discord")]
pub mod discord;
#[cfg(feature = "generic")]
pub mod generic;
#[cfg(feature = "slack")]
pub mod slack;
