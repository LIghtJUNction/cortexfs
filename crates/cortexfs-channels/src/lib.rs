//! Runtime-neutral channel abstractions for multi-platform agent messaging.
//!
//! The public boundary deliberately models conversations, threads, receipts,
//! capabilities, and health without importing FUSE, a model SDK, or an async
//! runtime. A host can register any object-safe [`ChannelAdapter`] and route
//! every inbound message into its own durable multi-turn session model.

mod adapter;
mod address;
mod capability;
mod error;
mod message;
pub mod platform;
mod registry;
mod route;
mod wire;

pub use adapter::{ChannelAdapter, ChannelFuture, ChannelStream, DeliveryReceipt};
pub use address::{ChannelId, ConversationId};
pub use capability::{ChannelCapabilities, ChannelHealth, HealthState};
pub use error::{ChannelError, RetryHint};
pub use message::{
    Attachment, InboundMessage, MessageBody, MessageTarget, OutboundMessage, Participant,
};
pub use platform::{ChannelCodec, OutboundRequest};
pub use registry::ChannelRegistry;
pub use route::ChannelSessionRoute;
pub use wire::{CHANNEL_ABI, ChannelEnvelope, ChannelEvent};

#[cfg(test)]
mod tests;
