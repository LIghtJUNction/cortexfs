use cortexfs_channels::{ChannelCapabilities, ChannelId, platform::email::EmailCodec};
use serde_json::Value;

use super::{EmailConfig, EmailError, smtp};
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
};

mod ops;

pub(super) fn start(
    config: &EmailConfig,
    bridge: &AgentChannelBridge,
) -> Result<ChannelControl, EmailError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("email"),
        bridge.clone(),
        ChannelCapabilities {
            receive: true,
            send: true,
            polling: true,
            long_polling: true,
            tool_control: true,
            ..ChannelCapabilities::empty()
        },
        cortexfs_channels::ChannelActions::empty(),
        Box::new(CodecHandler::new(Box::new(Transport {
            config: config.clone(),
        }))),
    ))
    .map_err(|error| EmailError::Config(error.to_string()))
}

struct Transport {
    config: EmailConfig,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn cortexfs_channels::ChannelCodec {
        &EmailCodec
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), host::ChannelControlError> {
        smtp::send_request(&self.config, &request).map_err(|error| fail(&error.to_string()))
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

fn fail(message: &str) -> host::ChannelControlError {
    host::ChannelControlError::Operation(message.to_owned())
}
