use cortexfs_channels::{ChannelCapabilities, ChannelId, platform::notion::NotionCodec};
use reqwest::blocking::Client;
use serde_json::Value;

use super::{NotionConfig, NotionError, api};
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
};

mod ops;

pub(super) fn start(
    config: &NotionConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
    codec: NotionCodec,
) -> Result<ChannelControl, NotionError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("notion"),
        bridge.clone(),
        ChannelCapabilities {
            receive: true,
            send: true,
            polling: true,
            tool_control: true,
            ..ChannelCapabilities::empty()
        },
        cortexfs_channels::ChannelActions::empty(),
        Box::new(CodecHandler::new(Box::new(Transport {
            client: client.clone(),
            config: config.clone(),
            codec,
        }))),
    ))
    .map_err(|error| NotionError::Protocol(error.to_string()))
}

struct Transport {
    client: Client,
    config: NotionConfig,
    codec: NotionCodec,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn cortexfs_channels::ChannelCodec {
        &self.codec
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), host::ChannelControlError> {
        api::send_outbound(&self.client, &self.config, &request)
            .map_err(|error| fail(&error.to_string()))
    }

    fn invoke(
        &mut self,
        target: Option<&cortexfs_channels::MessageTarget>,
        name: &str,
        payload: &Value,
    ) -> Result<Value, host::ChannelControlError> {
        ops::run(&self.client, &self.config, target, name, payload)
    }
}

fn fail(message: &str) -> host::ChannelControlError {
    host::ChannelControlError::Operation(message.to_owned())
}
