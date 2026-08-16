use crate::{ChannelError, InboundMessage, MessageTarget};

/// Deterministic mapping from a remote conversation to an agent session name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelSessionRoute {
    agent: String,
    prefix: String,
}

impl ChannelSessionRoute {
    pub fn new(agent: impl Into<String>, prefix: impl Into<String>) -> Result<Self, ChannelError> {
        let agent = agent.into();
        let prefix = prefix.into();
        if !valid_name(&agent) || !valid_name(&prefix) {
            return Err(ChannelError::InvalidValue(format!("{agent}/{prefix}")));
        }
        Ok(Self { agent, prefix })
    }

    #[must_use]
    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// Returns a stable, filesystem-safe session name for one conversation thread.
    #[must_use]
    pub fn session_for(&self, target: &MessageTarget) -> String {
        let key = conversation_key(target);
        format!("{}-{:016x}", self.prefix, fnv1a(key.as_bytes()))
    }

    /// Returns a deterministic idempotency key for one inbound platform message.
    #[must_use]
    pub fn request_id_for(&self, message: &InboundMessage) -> String {
        let mut key = conversation_key(&message.target);
        key.push('\0');
        key.push_str(&message.id);
        format!("im-{}-{:016x}", self.prefix, fnv1a(key.as_bytes()))
    }
}

fn conversation_key(target: &MessageTarget) -> String {
    let mut key = String::with_capacity(target.conversation.as_str().len() + 64);
    key.push_str(target.channel.as_str());
    key.push('\0');
    key.push_str(target.conversation.as_str());
    if let Some(thread) = target.thread.as_deref() {
        key.push('\0');
        key.push_str(thread);
    }
    key
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
