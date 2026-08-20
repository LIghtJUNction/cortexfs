use cortexfs_channels::{ChannelCapabilities, ChannelId, platform::dingtalk::DingTalkCodec};
use reqwest::blocking::Client;
use serde_json::Value;

use super::{DingTalkConfig, DingTalkError, Webhooks};
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
};

mod ops;
mod sign;

pub(super) fn start(
    config: &DingTalkConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
    webhooks: Webhooks,
) -> Result<ChannelControl, DingTalkError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("dingtalk"),
        bridge.clone(),
        ChannelCapabilities {
            receive: true,
            send: true,
            group: true,
            websocket: true,
            tool_control: true,
            ..ChannelCapabilities::empty()
        },
        cortexfs_channels::ChannelActions::empty(),
        Box::new(CodecHandler::new(Box::new(Transport {
            client: client.clone(),
            config: config.clone(),
            webhooks,
        }))),
    ))
    .map_err(|error| DingTalkError::Protocol(error.to_string()))
}

struct Transport {
    client: Client,
    config: DingTalkConfig,
    webhooks: Webhooks,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn cortexfs_channels::ChannelCodec {
        &DingTalkCodec
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), host::ChannelControlError> {
        ops::send(&self.client, &self.webhooks, request)
    }

    fn invoke(
        &mut self,
        target: Option<&cortexfs_channels::MessageTarget>,
        name: &str,
        payload: &Value,
    ) -> Result<Value, host::ChannelControlError> {
        ops::run(
            &self.client,
            &self.config,
            &self.webhooks,
            target,
            name,
            payload,
        )
    }
}
