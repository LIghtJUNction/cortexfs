#![expect(
    clippy::redundant_pub_crate,
    reason = "provider decoding is private driver plumbing"
)]

use std::collections::BTreeMap;

use crate::{config::Config, error::Result};
use cortexfs_channels::{
    ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget, Participant,
};

use super::{ActiveCall, Calls};

mod fields;

use fields::{first, form_value, terminal};

pub(crate) fn decode(
    config: &Config,
    content_type: &str,
    body: &str,
    calls: &mut Calls,
) -> Result<Option<InboundMessage>> {
    let value =
        if content_type.contains("x-www-form-urlencoded") || !body.trim_start().starts_with('{') {
            form_value(body)
        } else {
            serde_json::from_str(body)?
        };
    let call_id = first(
        &value,
        &[
            &["CallSid"],
            &["call_id"],
            &["data", "payload", "call_control_id"],
        ],
    )
    .unwrap_or("unknown-call");
    let sender = first(
        &value,
        &[&["From"], &["from"], &["data", "payload", "from"]],
    )
    .unwrap_or_default();
    if sender.is_empty() || !config.accepts(sender) {
        return Ok(None);
    }
    let event = first(
        &value,
        &[
            &["event_type"],
            &["status"],
            &["CallStatus"],
            &["data", "event_type"],
        ],
    )
    .unwrap_or("message");
    let text = first(
        &value,
        &[
            &["text"],
            &["transcript"],
            &["Transcript"],
            &["speech"],
            &["Speech"],
            &["transcription"],
            &["data", "payload", "text"],
            &["data", "payload", "transcript"],
            &["data", "payload", "transcription"],
        ],
    );
    if text.is_none() && terminal(event) {
        calls.retain(|_, call| call.id != call_id);
        return Ok(None);
    }
    let conversation = format!("call:{call_id}");
    let active = ActiveCall {
        id: call_id.to_owned(),
    };
    calls.insert(conversation.clone(), active.clone());
    calls.insert(format!("phone:{sender}"), active);
    let content = text.map_or_else(|| "voice event received".to_owned(), str::to_owned);
    let mut metadata = BTreeMap::new();
    metadata.insert("voice_call_id".to_owned(), call_id.to_owned());
    metadata.insert("voice_destination".to_owned(), sender.to_owned());
    metadata.insert("voice_event".to_owned(), event.to_owned());
    Ok(Some(InboundMessage {
        id: format!("voice-{call_id}-{}", content.len()),
        target: MessageTarget {
            channel: ChannelId::new(config.channel.id())?,
            conversation: ConversationId::new(conversation)?,
            thread: None,
            reply_to: None,
        },
        sender: Participant {
            id: sender.to_owned(),
            ..Participant::default()
        },
        body: MessageBody::text(content)?,
        timestamp_ms: None,
        metadata,
    }))
}

#[cfg(test)]
mod tests;
