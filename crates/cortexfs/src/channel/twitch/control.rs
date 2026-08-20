use std::{io::Write, net::TcpStream};

use cortexfs_channels::{ChannelCapabilities, ChannelId, platform::twitch::TwitchCodec};
use serde_json::Value;

use super::{TwitchConfig, TwitchError, tls};
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
    irc::wire,
};

mod ops;

pub(super) fn start(
    config: &TwitchConfig,
    bridge: &AgentChannelBridge,
) -> Result<ChannelControl, TwitchError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("twitch"),
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
    .map_err(|error| TwitchError::Tls(error.to_string()))
}

struct Transport {
    config: TwitchConfig,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn cortexfs_channels::ChannelCodec {
        &TwitchCodec
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
        send(&self.config, &ops::line(target, name, payload)?)?;
        Ok(serde_json::json!({"accepted":true}))
    }
}

fn send(config: &TwitchConfig, body: &str) -> Result<(), host::ChannelControlError> {
    let tcp = TcpStream::connect((&*config.server, config.port))
        .map_err(|error| fail(&error.to_string()))?;
    let mut stream = tls::connect(tcp, &config.server).map_err(|error| fail(&error))?;
    line(
        &mut stream,
        "CAP REQ :twitch.tv/membership twitch.tv/tags twitch.tv/commands",
    )?;
    line(
        &mut stream,
        &format!(
            "PASS {}",
            cortexfs_channels::platform::twitch::normalize_oauth_token(&config.oauth_token)
        ),
    )?;
    line(&mut stream, &format!("NICK {}", config.nickname))?;
    line(
        &mut stream,
        &format!("USER {} 0 * :CortexFS", config.nickname),
    )?;
    for channel in &config.channels {
        line(&mut stream, &format!("JOIN {channel}"))?;
    }
    line(&mut stream, body)
}

fn line(stream: &mut impl Write, value: &str) -> Result<(), host::ChannelControlError> {
    wire::line(stream, &format!("{value}\r\n")).map_err(|error| fail(&error.to_string()))
}

fn fail(message: &str) -> host::ChannelControlError {
    host::ChannelControlError::Operation(message.to_owned())
}
