//! Runtime-neutral channel abstractions for multi-platform agent messaging.
//!
//! The public boundary deliberately models conversations, threads, receipts,
//! capabilities, and health without importing FUSE, a model SDK, or an async
//! runtime. A host can register any object-safe [`ChannelAdapter`] and route
//! every inbound message into its own durable multi-turn session model.

mod action;
mod adapter;
mod address;
mod capability;
mod command;
mod driver;
mod effect;
mod error;
mod event;
mod frame;
mod framewire;
mod incoming;
mod message;
pub mod platform;
mod progress;
mod registry;
mod route;
mod wire;

pub use action::{ChannelAction, ChannelActions};
pub use adapter::{
    ChannelAdapter, ChannelEventStream, ChannelFuture, ChannelIncomingStream, ChannelStream,
    DeliveryReceipt,
};
pub use address::{ChannelId, ConversationId};
pub use capability::{ChannelCapabilities, ChannelHealth, HealthState};
pub use command::{ChannelChoice, ChannelCommand, ChannelCommandResult};
pub use driver::{ChannelDriverClient, ChannelDriverError, ChannelDriverSession};
pub use effect::ChannelEffect;
pub use error::{ChannelError, RetryHint};
pub use event::{ChannelEventContext, ChannelIncomingEvent};
pub use frame::{CHANNEL_SOCKET_ABI, ChannelFrame, ChannelFrameBody, ChannelRuntimeEvent};
pub use framewire::{ChannelWireError, MAX_CHANNEL_FRAME_BYTES};
pub use incoming::ChannelIncoming;
pub use message::{
    Attachment, InboundMessage, MessageBody, MessageTarget, OutboundMessage, Participant,
};
pub use platform::catalog::{CHANNEL_CATALOG, ChannelSpec, ChannelTransport};
pub use platform::{ChannelCodec, OutboundRequest};
pub use progress::ChannelProgressPolicy;
pub use registry::ChannelRegistry;
pub use route::ChannelSessionRoute;
pub use wire::{CHANNEL_ABI, ChannelEnvelope, ChannelEvent};

#[cfg(test)]
mod tests;
