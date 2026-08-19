use serde::{Deserialize, Serialize};

use crate::ChannelIncomingEvent;
use crate::{
    ChannelActions, ChannelCapabilities, ChannelCommand, ChannelCommandResult,
    ChannelControlAction, ChannelEffect, ChannelHealth, ChannelId, DeliveryReceipt, InboundMessage,
    MessageTarget, OutboundMessage,
};

/// Versioned JSONL protocol for a bidirectional channel driver socket.
pub const CHANNEL_SOCKET_ABI: &str = "cortexfs.channel.socket/v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelFrame {
    pub abi: String,
    pub frame: ChannelFrameBody,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelFrameBody {
    Hello {
        request_id: String,
        channel: ChannelId,
        capabilities: ChannelCapabilities,
        #[serde(default)]
        actions: ChannelActions,
    },
    ControlHello {
        request_id: String,
        channel: ChannelId,
    },
    Start {
        request_id: String,
    },
    Stop {
        request_id: String,
    },
    Inbound {
        event_id: String,
        message: InboundMessage,
    },
    InboundEvent {
        event_id: String,
        event: ChannelIncomingEvent,
    },
    Deliver {
        request_id: String,
        message: OutboundMessage,
    },
    /// Runtime-initiated delivery that is not tied to a preceding Inbound.
    Outbound {
        request_id: String,
        message: OutboundMessage,
    },
    Effect {
        request_id: String,
        target: MessageTarget,
        effect: ChannelEffect,
    },
    Command {
        request_id: String,
        session: String,
        command_id: String,
        command: ChannelCommand,
        /// Destination needed by adapters that present the command remotely.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<MessageTarget>,
    },
    CommandResult {
        request_id: String,
        session: String,
        command_id: String,
        result: ChannelCommandResult,
    },
    ControlRequest {
        request_id: String,
        action: ChannelControlAction,
    },
    ControlResponse {
        request_id: String,
        accepted: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Receipt {
        request_id: String,
        receipt: DeliveryReceipt,
    },
    HealthRequest {
        request_id: String,
    },
    HealthResponse {
        request_id: String,
        health: ChannelHealth,
    },
    Health {
        health: ChannelHealth,
    },
    Event {
        event: ChannelRuntimeEvent,
    },
    Error {
        request_id: Option<String>,
        code: String,
        message: String,
        retryable: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelRuntimeEvent {
    Connected,
    Disconnected,
    RetryScheduled { attempt: u32 },
    Heartbeat,
}

impl ChannelFrame {
    #[must_use]
    pub fn new(frame: ChannelFrameBody) -> Self {
        Self {
            abi: CHANNEL_SOCKET_ABI.to_owned(),
            frame,
        }
    }
}
