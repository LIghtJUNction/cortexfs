use crate::{ChannelError, ChannelIncomingEvent, InboundMessage, MessageTarget};
use std::collections::BTreeSet;

mod authorization;

/// Deterministic mapping from a remote conversation to an agent session name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelSessionRoute {
    agent: String,
    prefix: String,
    isolate_identity: bool,
    allowed_senders: BTreeSet<String>,
}

impl ChannelSessionRoute {
    pub fn new(agent: impl Into<String>, prefix: impl Into<String>) -> Result<Self, ChannelError> {
        let agent = agent.into();
        let prefix = prefix.into();
        if !valid_name(&agent) || !valid_name(&prefix) {
            return Err(ChannelError::InvalidValue(format!("{agent}/{prefix}")));
        }
        Ok(Self {
            agent,
            prefix,
            isolate_identity: false,
            allowed_senders: BTreeSet::new(),
        })
    }

    #[must_use]
    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// Includes the external sender in derived sessions for multi-user hosts.
    #[must_use]
    pub const fn with_identity_isolation(mut self) -> Self {
        self.isolate_identity = true;
        self
    }
    /// Returns a stable, filesystem-safe session name for one conversation thread.
    #[must_use]
    pub fn session_for(&self, target: &MessageTarget) -> String {
        let key = conversation_key(target);
        format!("{}-{:016x}", self.prefix, fnv1a(key.as_bytes()))
    }
    /// Returns a session key that can isolate members of one group.
    #[must_use]
    pub fn session_for_message(&self, message: &InboundMessage) -> String {
        let key = self.message_key(message);
        format!("{}-{:016x}", self.prefix, fnv1a(key.as_bytes()))
    }
    /// Returns a stable session name for a non-message event.
    #[must_use]
    pub fn session_for_event(&self, event: &ChannelIncomingEvent) -> String {
        let key = self.event_key(event);
        format!("{}-{:016x}", self.prefix, fnv1a(key.as_bytes()))
    }
    /// Returns a deterministic idempotency key for one inbound platform message.
    #[must_use]
    pub fn request_id_for(&self, message: &InboundMessage) -> String {
        let mut key = self.message_key(message);
        key.push('\0');
        key.push_str(&message.id);
        format!("im-{}-{:016x}", self.prefix, fnv1a(key.as_bytes()))
    }

    /// Returns a stable idempotency key for a non-message event.
    #[must_use]
    pub fn request_id_for_event(&self, event: &ChannelIncomingEvent) -> String {
        let mut key = self.event_key(event);
        key.push('\0');
        key.push_str(&serde_json::to_string(event).unwrap_or_else(|_| format!("{event:?}")));
        format!("im-{}-{:016x}", self.prefix, fnv1a(key.as_bytes()))
    }

    fn message_key(&self, message: &InboundMessage) -> String {
        if self.isolate_identity && !message.sender.id.is_empty() {
            let mut key = conversation_key(&message.target);
            key.push('\0');
            key.push_str(&message.sender.id);
            key
        } else {
            conversation_key(&message.target)
        }
    }

    fn event_key(&self, event: &ChannelIncomingEvent) -> String {
        let mut key = conversation_key(&event.context().target);
        if self.isolate_identity
            && let Some(participant) = event.context().participant.as_ref()
            && !participant.id.is_empty()
        {
            key.push('\0');
            key.push_str(&participant.id);
        }
        key
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

#[expect(
    clippy::redundant_pub_crate,
    reason = "the private route module shares this hash with sibling ABI helpers"
)]
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
