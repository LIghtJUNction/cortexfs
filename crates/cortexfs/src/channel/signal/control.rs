use cortexfs_channels::{ChannelCapabilities, ChannelId, platform::signal::SignalCodec};
use serde_json::Value;

use super::{SignalConfig, SignalError};
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
};

mod ops;

pub(super) fn start(
    config: &SignalConfig,
    bridge: &AgentChannelBridge,
) -> Result<ChannelControl, SignalError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("signal"),
        bridge.clone(),
        ChannelCapabilities {
            receive: true,
            send: true,
            tool_control: true,
            ..ChannelCapabilities::empty()
        },
        cortexfs_channels::ChannelActions::empty(),
        Box::new(CodecHandler::new(Box::new(Transport {
            config: config.clone(),
        }))),
    ))
    .map_err(|error| SignalError::Config(error.to_string()))
}

struct Transport {
    config: SignalConfig,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn cortexfs_channels::ChannelCodec {
        &SignalCodec
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), host::ChannelControlError> {
        ops::send_request(&self.config, &request)
    }

    fn invoke(
        &mut self,
        target: Option<&cortexfs_channels::MessageTarget>,
        name: &str,
        payload: &Value,
    ) -> Result<Value, host::ChannelControlError> {
        ops::run(&self.config, target, name, payload)
    }
}
