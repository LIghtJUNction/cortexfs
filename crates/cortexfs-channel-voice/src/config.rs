#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "configuration is shared by private driver modules"
)]

use std::{collections::BTreeSet, env, net::SocketAddr, path::PathBuf, time::Duration};

use cortexfs_channels::ChannelCapabilities;

use crate::error::{Error, Result};

mod parse;
use parse::{list, optional, provider, required, seconds};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChannelKind {
    VoiceCall,
    ClawdTalk,
    VoiceWake,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Provider {
    Twilio,
    Telnyx,
    Plivo,
}

pub(crate) struct Config {
    pub(crate) channel: ChannelKind,
    pub(crate) provider: Provider,
    pub(crate) api_base: String,
    pub(crate) auth_token: String,
    pub(crate) account_id: String,
    pub(crate) from_number: String,
    pub(crate) allowed_destinations: BTreeSet<String>,
    pub(crate) socket: PathBuf,
    pub(crate) webhook_bind: SocketAddr,
    pub(crate) webhook_token: Option<String>,
    pub(crate) webhook_base: Option<String>,
    pub(crate) hangup_after: Option<Duration>,
    pub(crate) wake_executable: Option<String>,
}

impl Config {
    pub(crate) fn load() -> Result<Self> {
        let channel = match env::var("CORTEXFS_VOICE_CHANNEL").as_deref() {
            Ok("clawdtalk") => ChannelKind::ClawdTalk,
            Ok("voice_wake") => ChannelKind::VoiceWake,
            Ok("voice_call") | Err(_) => ChannelKind::VoiceCall,
            Ok(value) => return Err(Error::Config(format!("unknown channel: {value}"))),
        };
        let provider = provider(env::var("CORTEXFS_VOICE_PROVIDER").ok().as_deref())?;
        if channel == ChannelKind::ClawdTalk && provider != Provider::Telnyx {
            return Err(Error::Config("clawdtalk requires telnyx".to_owned()));
        }
        let channel_id = channel.id();
        let expected = cortexfs_paths::channel_driver_socket(channel_id);
        let socket = PathBuf::from(
            env::var("CORTEXFS_CHANNEL_SOCKET").unwrap_or_else(|_| expected.display().to_string()),
        );
        if socket != expected {
            return Err(Error::Config(format!(
                "CORTEXFS_CHANNEL_SOCKET must be {}",
                expected.display()
            )));
        }
        let base =
            env::var("CORTEXFS_VOICE_API_BASE").unwrap_or_else(|_| provider.base().to_owned());
        let webhook_bind = env::var("CORTEXFS_VOICE_WEBHOOK_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8789".to_owned())
            .parse()
            .map_err(|error| Error::Config(format!("invalid webhook bind address: {error}")))?;
        let wake = channel == ChannelKind::VoiceWake;
        Ok(Self {
            channel,
            provider,
            api_base: base.trim_end_matches('/').to_owned(),
            auth_token: if wake {
                String::new()
            } else {
                required("CORTEXFS_VOICE_AUTH_TOKEN")?
            },
            account_id: if wake {
                String::new()
            } else {
                required("CORTEXFS_VOICE_ACCOUNT_ID")?
            },
            from_number: if wake {
                String::new()
            } else {
                required("CORTEXFS_VOICE_FROM_NUMBER")?
            },
            allowed_destinations: if wake {
                BTreeSet::new()
            } else {
                list("CORTEXFS_VOICE_ALLOWED_DESTINATIONS")
            },
            socket,
            webhook_bind,
            webhook_token: optional("CORTEXFS_VOICE_WEBHOOK_TOKEN"),
            webhook_base: optional("CORTEXFS_VOICE_WEBHOOK_BASE_URL"),
            hangup_after: seconds("CORTEXFS_VOICE_HANGUP_AFTER_SECONDS")?,
            wake_executable: optional("CORTEXFS_VOICE_WAKE_EXECUTABLE"),
        })
    }

    pub(crate) fn accepts(&self, destination: &str) -> bool {
        self.allowed_destinations
            .iter()
            .any(|value| value == "*" || value == destination)
    }

    pub(crate) const fn capabilities(channel: ChannelKind) -> ChannelCapabilities {
        match channel {
            ChannelKind::VoiceWake => ChannelCapabilities {
                audio: true,
                tool_control: true,
                ..ChannelCapabilities::empty()
            },
            ChannelKind::VoiceCall | ChannelKind::ClawdTalk => ChannelCapabilities {
                audio: true,
                webhook: true,
                tool_control: true,
                ..ChannelCapabilities::text()
            },
        }
    }
}

impl ChannelKind {
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::VoiceCall => "voice_call",
            Self::ClawdTalk => "clawdtalk",
            Self::VoiceWake => "voice_wake",
        }
    }
}

impl Provider {
    const fn base(self) -> &'static str {
        match self {
            Self::Twilio => "https://api.twilio.com/2010-04-01",
            Self::Telnyx => "https://api.telnyx.com/v2",
            Self::Plivo => "https://api.plivo.com/v1",
        }
    }
}
