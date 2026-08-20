use cortexfs_channels::{ChannelCapabilities, ChannelId, platform::gmail::GmailCodec};
use reqwest::blocking::Client;
use serde_json::Value;

use super::{GmailConfig, GmailError, api};
use cortexfs::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
};

mod ops;

pub(super) fn start(
    config: &GmailConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<ChannelControl, GmailError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("gmail"),
        bridge.clone(),
        ChannelCapabilities {
            receive: true,
            send: true,
            webhook: true,
            tool_control: true,
            ..ChannelCapabilities::empty()
        },
        cortexfs_channels::ChannelActions::empty(),
        Box::new(CodecHandler::new(Box::new(Transport {
            client: client.clone(),
            config: config.clone(),
        }))),
    ))
    .map_err(|error| GmailError::Api(error.to_string()))
}

struct Transport {
    client: Client,
    config: GmailConfig,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn cortexfs_channels::ChannelCodec {
        &GmailCodec
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), host::ChannelControlError> {
        api::GmailApi::new(
            &self.client,
            &self.config.api_base,
            &self.config.access_token,
        )
        .send(request)
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
