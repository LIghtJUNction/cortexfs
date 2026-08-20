#![forbid(unsafe_code)]

//! SDK for process-isolated `CortexFS` channel adapters.

mod error;
mod runtime;
mod sender;
mod service;

pub use cortexfs_channels::{
    Attachment, ChannelAction, ChannelActions, ChannelCapabilities, ChannelChoice, ChannelCommand,
    ChannelCommandResult, ChannelEffect, ChannelError, ChannelHealth, ChannelId, ChannelIncoming,
    ChannelRuntimeEvent, ConversationId, DeliveryReceipt, InboundMessage, MessageBody,
    MessageTarget, OutboundMessage, Participant,
};
pub use error::ChannelSdkError;
pub use runtime::ChannelRuntime;
pub use sender::ChannelSender;
pub use service::ChannelService;

#[cfg(test)]
mod tests;
