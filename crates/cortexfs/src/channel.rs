//! L5 channel bridge: transport adapters feed the existing agent session socket.

pub mod bridge;
pub mod discord;
pub mod event;
pub mod http;
pub mod telegram;

#[cfg(test)]
mod tests;
