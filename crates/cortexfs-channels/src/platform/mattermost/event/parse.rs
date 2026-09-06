use std::collections::BTreeMap;

use serde_json::Value;

use super::super::super::{participant, scalar};
use crate::{ChannelError, ChannelEventContext, ChannelId, ConversationId, MessageTarget};

pub(super) fn context(
    root: &Value,
    data: &Value,
    post: Option<&Value>,
    actor: Option<&Value>,
    channel: ChannelId,
) -> Result<ChannelEventContext, ChannelError> {
    let conversation = ConversationId::new(scalar(
        field(data, post, None, "channel_id").or_else(|| root.pointer("/broadcast/channel_id")),
        "channel_id",
    )?)?;
    let participant = actor
        .and_then(|value| value.get("user_id"))
        .map(|value| scalar(Some(value), "user_id"))
        .transpose()?
        .map(|id| participant(None, id));
    let thread = field(data, post, actor, "parent_id")
        .or_else(|| field(data, post, actor, "root_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    Ok(ChannelEventContext {
        target: MessageTarget {
            channel,
            conversation,
            thread,
            reply_to: None,
        },
        participant,
        timestamp_ms: field(data, post, actor, "create_at").and_then(Value::as_u64),
        metadata: BTreeMap::default(),
    })
}

fn field<'a>(
    data: &'a Value,
    post: Option<&'a Value>,
    actor: Option<&'a Value>,
    name: &str,
) -> Option<&'a Value> {
    data.get(name)
        .or_else(|| post.and_then(|value| value.get(name)))
        .or_else(|| actor.and_then(|value| value.get(name)))
}

#[expect(
    clippy::pattern_type_mismatch,
    reason = "embedded provider fields are inspected without moving the payload"
)]
pub(super) fn embedded(value: Option<&Value>) -> Option<Value> {
    match value {
        Some(Value::String(value)) => serde_json::from_str(value).ok(),
        Some(value) if value.is_object() => Some(value.clone()),
        _ => None,
    }
}
