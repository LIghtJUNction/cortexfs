use std::{fmt, thread, time::Duration};

use super::bridge::{AgentChannelBridge, ChannelBridgeError};
use cortexfs_channels::ChannelError;

mod imap;
mod smtp;

/// IMAP IDLE plus SMTP channel configuration.
pub struct EmailConfig {
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
    pub mailbox: String,
    pub idle_seconds: u64,
}

impl fmt::Debug for EmailConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmailConfig")
            .field("imap_host", &self.imap_host)
            .field("imap_port", &self.imap_port)
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .field("from", &self.from)
            .field("mailbox", &self.mailbox)
            .field("idle_seconds", &self.idle_seconds)
            .finish()
    }
}

impl EmailConfig {
    fn validate(&self) -> Result<(), EmailError> {
        if self.imap_host.is_empty()
            || self.smtp_host.is_empty()
            || self.username.is_empty()
            || self.password.is_empty()
            || self.from.is_empty()
            || self.mailbox.is_empty()
        {
            return Err(EmailError::Config(
                "email connection fields are required".to_owned(),
            ));
        }
        if self.idle_seconds == 0 || self.idle_seconds > 1_740 {
            return Err(EmailError::Config(
                "idle interval must be between 1 and 1740 seconds".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("email configuration failed: {0}")]
    Config(String),
    #[error("IMAP operation failed: {0}")]
    Imap(String),
    #[error("SMTP operation failed: {0}")]
    Smtp(String),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
}

/// Runs IMAP IDLE and reconnects after a transport failure.
pub fn run(config: &EmailConfig, bridge: &AgentChannelBridge) -> Result<(), EmailError> {
    config.validate()?;
    loop {
        if let Err(_error) = imap::run_once(config, bridge) {
            thread::sleep(Duration::from_secs(5));
        }
    }
}
