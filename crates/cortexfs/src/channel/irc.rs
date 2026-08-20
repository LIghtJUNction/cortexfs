use std::{fmt, net::TcpStream, thread, time::Duration};

use cortexfs_channels::{ChannelError, platform::irc::IrcCodec};

use super::bridge::{AgentChannelBridge, ChannelBridgeError};

mod runner;
pub(in crate::channel) mod wire;

mod control;

pub(in crate::channel) use runner::{run_stream, run_stream_with};

/// Plain IRC foreground adapter. TLS can be supplied by an external local relay.
#[derive(Clone)]
pub struct IrcConfig {
    pub server: String,
    pub port: u16,
    pub nickname: String,
    pub username: String,
    pub channels: Vec<String>,
    pub password: Option<String>,
}

impl fmt::Debug for IrcConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IrcConfig")
            .field("server", &self.server)
            .field("port", &self.port)
            .field("nickname", &self.nickname)
            .field("username", &self.username)
            .field("channels", &self.channels)
            .field("password", &self.password.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl IrcConfig {
    pub fn new(
        server: String,
        port: u16,
        nickname: String,
        channels: Vec<String>,
    ) -> Result<Self, IrcError> {
        if server.is_empty() || nickname.is_empty() || channels.is_empty() {
            return Err(IrcError::Config(
                "server, nickname, and channels are required".to_owned(),
            ));
        }
        Ok(Self {
            username: nickname.clone(),
            server,
            port,
            nickname,
            channels,
            password: None,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IrcError {
    #[error("IRC configuration failed: {0}")]
    Config(String),
    #[error("IRC I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
}

/// Runs an IRC connection and reconnects after a disconnect.
pub fn run(config: &IrcConfig, bridge: &AgentChannelBridge) -> Result<(), IrcError> {
    let control = control::start(config, bridge)?;
    loop {
        control
            .check()
            .map_err(|error| IrcError::Config(error.to_string()))?;
        if let Err(_error) = run_once(config, bridge) {
            thread::sleep(Duration::from_secs(5));
        }
    }
}

fn run_once(config: &IrcConfig, bridge: &AgentChannelBridge) -> Result<(), IrcError> {
    let stream = TcpStream::connect((&*config.server, config.port))?;
    stream.set_read_timeout(Some(Duration::from_mins(5)))?;
    run_stream(config, stream, &IrcCodec, bridge)
}
