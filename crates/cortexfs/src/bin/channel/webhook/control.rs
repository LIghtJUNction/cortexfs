use std::sync::Arc;

use cortexfs_channels::{ChannelCodec, MessageTarget};
use reqwest::blocking::Client;
use serde_json::Value;

use super::WebhookConfig;
use super::outbound;
use cortexfs::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
};

mod native;
mod ops;

pub(super) fn start(
    config: &WebhookConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
    codec: Arc<dyn ChannelCodec>,
) -> Result<ChannelControl, super::WebhookError> {
    let channel = config.channel.clone().unwrap_or_else(|| codec.channel());
    let mut capabilities = codec.capabilities();
    capabilities.tool_control = true;
    host::start(ChannelControlConfig::new(
        channel,
        bridge.clone(),
        capabilities,
        codec.actions(),
        Box::new(CodecHandler::new(Box::new(Transport {
            client: client.clone(),
            config: config.clone(),
            codec,
        }))),
    ))
    .map_err(|error| super::WebhookError::Control(error.to_string()))
}

struct Transport {
    client: Client,
    config: WebhookConfig,
    codec: Arc<dyn ChannelCodec>,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn ChannelCodec {
        self.codec.as_ref()
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), cortexfs::channel::control::ChannelControlError> {
        outbound::send(&self.client, &self.config, request).map_err(|error| {
            cortexfs::channel::control::ChannelControlError::Operation(error.to_string())
        })
    }

    fn invoke(
        &mut self,
        target: Option<&MessageTarget>,
        name: &str,
        payload: &Value,
    ) -> Result<Value, cortexfs::channel::control::ChannelControlError> {
        ops::run(
            &self.client,
            &self.config,
            self.codec.as_ref(),
            target,
            name,
            payload,
        )
    }
}
