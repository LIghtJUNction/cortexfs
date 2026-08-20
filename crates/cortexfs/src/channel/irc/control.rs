use std::net::TcpStream;

use cortexfs_channels::{ChannelCapabilities, ChannelId, platform::irc::IrcCodec};
use serde_json::Value;

use super::{IrcConfig, IrcError, wire};
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
};

mod ops;

pub(super) fn start(
    config: &IrcConfig,
    bridge: &AgentChannelBridge,
) -> Result<ChannelControl, IrcError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("irc"),
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
    .map_err(|error| IrcError::Config(error.to_string()))
}

struct Transport {
    config: IrcConfig,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn cortexfs_channels::ChannelCodec {
        &IrcCodec
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), host::ChannelControlError> {
        send(&self.config, &request.body)
    }

    fn invoke(
        &mut self,
        target: Option<&cortexfs_channels::MessageTarget>,
        name: &str,
        payload: &Value,
    ) -> Result<Value, host::ChannelControlError> {
        let line = ops::line(target, name, payload)?;
        send(&self.config, &line)?;
        Ok(serde_json::json!({"accepted":true}))
    }
}

fn send(config: &IrcConfig, body: &str) -> Result<(), host::ChannelControlError> {
    let mut stream = TcpStream::connect((&*config.server, config.port))
        .map_err(|error| fail(&error.to_string()))?;
    if let Some(password) = config.password.as_deref() {
        wire::line(&mut stream, &format!("PASS {password}\r\n"))
            .map_err(|error| operation(&error))?;
    }
    wire::line(&mut stream, &format!("NICK {}\r\n", config.nickname))
        .map_err(|error| operation(&error))?;
    wire::line(
        &mut stream,
        &format!("USER {} 0 * :CortexFS\r\n", config.username),
    )
    .map_err(|error| operation(&error))?;
    for channel in &config.channels {
        wire::line(&mut stream, &format!("JOIN {channel}\r\n"))
            .map_err(|error| operation(&error))?;
    }
    wire::line(&mut stream, body).map_err(|error| operation(&error))
}

fn operation(error: &IrcError) -> host::ChannelControlError {
    fail(&error.to_string())
}

fn fail(message: &str) -> host::ChannelControlError {
    host::ChannelControlError::Operation(message.to_owned())
}
