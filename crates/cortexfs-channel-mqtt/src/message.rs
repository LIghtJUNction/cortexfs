#![expect(
    clippy::redundant_pub_crate,
    reason = "message conversion is private driver plumbing"
)]

use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage, Participant,
};
use rumqttc::AsyncClient;
use serde_json::Value;

use crate::{config::Config, error::Result};

pub(crate) fn decode(topic: &str, payload: &[u8]) -> Result<InboundMessage> {
    let raw = std::str::from_utf8(payload)
        .map_err(|_error| ChannelError::InvalidMessage("MQTT payload is not UTF-8".to_owned()))?;
    let root = serde_json::from_str::<Value>(raw).ok();
    let text = root
        .as_ref()
        .and_then(|value| value.get("text").or_else(|| value.get("body")))
        .and_then(Value::as_str)
        .unwrap_or(raw)
        .to_owned();
    let conversation = field(root.as_ref(), &["conversation", "conversation_id"])
        .unwrap_or_else(|| topic.to_owned());
    let sender = field(root.as_ref(), &["sender", "sender_id", "author"])
        .unwrap_or_else(|| "mqtt".to_owned());
    let id = field(root.as_ref(), &["id", "message_id"])
        .unwrap_or_else(|| format!("mqtt-{:016x}", hash(topic, raw)));
    let target = MessageTarget {
        channel: ChannelId::from_static("mqtt"),
        conversation: ConversationId::new(conversation)?,
        thread: field(root.as_ref(), &["thread", "thread_id"]),
        reply_to: field(root.as_ref(), &["reply_to", "reply_to_id"]),
    };
    let mut metadata = BTreeMap::new();
    metadata.insert("mqtt.topic".to_owned(), topic.to_owned());
    Ok(InboundMessage {
        id,
        target,
        sender: Participant {
            id: sender,
            ..Participant::default()
        },
        body: MessageBody::text(text)?,
        timestamp_ms: root
            .as_ref()
            .and_then(|value| value.get("timestamp_ms"))
            .and_then(Value::as_u64),
        metadata,
    })
}

pub(crate) async fn publish(
    client: &AsyncClient,
    config: &Config,
    message: &OutboundMessage,
) -> Result<String> {
    message.body.validate()?;
    if !message.body.attachments.is_empty() {
        return Err(ChannelError::Unsupported("MQTT attachments".to_owned()).into());
    }
    let topic = message
        .metadata
        .get("mqtt.topic")
        .or(config.outbound_topic.as_ref())
        .map_or(message.target.conversation.as_str(), String::as_str);
    if topic.is_empty() || topic.contains('#') || topic.contains('+') {
        return Err(ChannelError::InvalidValue(topic.to_owned()).into());
    }
    client
        .publish(topic, config.qos, false, message.body.text.clone())
        .await?;
    Ok(topic.to_owned())
}

fn field(root: Option<&Value>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        root.and_then(|value| value.get(*name))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn hash(topic: &str, payload: &str) -> u64 {
    topic
        .bytes()
        .chain([0])
        .chain(payload.bytes())
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}
