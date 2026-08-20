use cortexfs_channels::{ChannelCapabilities, ChannelId, platform::bluesky::BlueskyCodec};
use reqwest::blocking::Client;
use serde_json::Value;

use super::{BlueskyConfig, BlueskyError, api};
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
};

mod ops;

pub(super) fn start(
    config: &BlueskyConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<ChannelControl, BlueskyError> {
    let session = api::login(client, config)?;
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("bluesky"),
        bridge.clone(),
        ChannelCapabilities {
            send: true,
            threads: true,
            polling: true,
            tool_control: true,
            ..ChannelCapabilities::empty()
        },
        cortexfs_channels::ChannelActions::empty(),
        Box::new(CodecHandler::new(Box::new(Transport {
            client: client.clone(),
            config: config.clone(),
            session,
        }))),
    ))
    .map_err(|error| BlueskyError::Protocol(error.to_string()))
}

struct Transport {
    client: Client,
    config: BlueskyConfig,
    session: api::Session,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn cortexfs_channels::ChannelCodec {
        &BlueskyCodec
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), host::ChannelControlError> {
        api::send(&self.client, &self.config, &mut self.session, request)
            .map_err(|error| fail(&error.to_string()))
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
            &mut self.session,
            target,
            name,
            payload,
        )
    }
}

fn fail(message: &str) -> host::ChannelControlError {
    host::ChannelControlError::Operation(message.to_owned())
}
