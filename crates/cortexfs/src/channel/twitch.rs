use std::{fmt, net::TcpStream, thread, time::Duration};

use cortexfs_channels::platform::twitch::TwitchCodec;

use super::{
    bridge::AgentChannelBridge,
    irc::{self, IrcConfig, IrcError},
};

mod control;
mod tls;

/// Twitch chat configuration using the platform's TLS IRC endpoint.
#[derive(Clone)]
pub struct TwitchConfig {
    pub server: String,
    pub port: u16,
    pub nickname: String,
    pub oauth_token: String,
    pub channels: Vec<String>,
    pub mention_only: bool,
}

impl fmt::Debug for TwitchConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwitchConfig")
            .field("server", &self.server)
            .field("port", &self.port)
            .field("nickname", &self.nickname)
            .field("oauth_token", &"[redacted]")
            .field("channels", &self.channels)
            .field("mention_only", &self.mention_only)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TwitchError {
    #[error(transparent)]
    Irc(#[from] IrcError),
    #[error("Twitch TLS connection failed: {0}")]
    Tls(String),
}

pub fn run(config: &TwitchConfig, bridge: &AgentChannelBridge) -> Result<(), TwitchError> {
    let control = control::start(config, bridge)?;
    loop {
        control
            .check()
            .map_err(|error| TwitchError::Tls(error.to_string()))?;
        if let Err(_error) = run_once(config, bridge) {
            thread::sleep(Duration::from_secs(5));
        }
    }
}

fn run_once(config: &TwitchConfig, bridge: &AgentChannelBridge) -> Result<(), TwitchError> {
    let stream = TcpStream::connect((&*config.server, config.port))
        .map_err(|error| TwitchError::Tls(error.to_string()))?;
    stream
        .set_read_timeout(Some(Duration::from_mins(5)))
        .map_err(|error| TwitchError::Tls(error.to_string()))?;
    let stream = tls::connect(stream, &config.server).map_err(TwitchError::Tls)?;
    let mut irc = IrcConfig::new(
        config.server.clone(),
        config.port,
        config.nickname.clone(),
        config.channels.clone(),
    )?;
    irc.password = Some(cortexfs_channels::platform::twitch::normalize_oauth_token(
        &config.oauth_token,
    ));
    let nickname = config.nickname.to_ascii_lowercase();
    irc::run_stream_with(
        &irc,
        stream,
        &TwitchCodec,
        bridge,
        &["CAP REQ :twitch.tv/membership twitch.tv/tags twitch.tv/commands"],
        move |message| {
            !config.mention_only
                || message
                    .body
                    .text
                    .to_ascii_lowercase()
                    .contains(&format!("@{nickname}"))
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::TwitchConfig;

    #[test]
    fn debug_redacts_oauth_token() {
        let config = TwitchConfig {
            server: "irc.chat.twitch.tv".to_owned(),
            port: 6697,
            nickname: "bot".to_owned(),
            oauth_token: "secret".to_owned(),
            channels: vec!["#room".to_owned()],
            mention_only: true,
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("secret"));
    }
}
