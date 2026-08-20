//! L5 channel bridge: transport adapters feed the existing agent session socket.

pub mod bluesky;
pub mod bridge;
#[doc(hidden)]
pub mod control;
pub mod dingtalk;
pub mod discord;
pub mod driver;
pub(crate) mod driverhandle;
mod driverprogress;
pub mod email;
pub mod event;
pub mod http;
pub mod irc;
pub mod matrix;
pub mod mattermost;
pub mod mochat;
pub mod notion;
pub(crate) mod progress;
pub mod qq;
pub mod reddit;
pub mod signal;
pub mod telegram;
pub mod twitch;
pub mod twitter;

#[cfg(test)]
mod tests;
