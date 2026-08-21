use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
};

use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage, OutboundRequest, Participant,
};

pub(in crate::platform) fn decode(
    channel: ChannelId,
    payload: &str,
) -> Result<Option<InboundMessage>, ChannelError> {
    let line = payload.trim_end_matches(['\r', '\n']);
    let (tags, line) = line.strip_prefix('@').map_or((None, line), |value| {
        value
            .split_once(' ')
            .map_or((Some(value), ""), |(tags, rest)| (Some(tags), rest))
    });
    let (prefix, rest) = line.strip_prefix(':').map_or((None, line), |value| {
        value
            .split_once(' ')
            .map_or((Some(value), ""), |(p, r)| (Some(p), r))
    });
    let (head, body) = rest.split_once(" :").unwrap_or((rest, ""));
    let mut fields = head.split_whitespace();
    if fields.next() != Some("PRIVMSG") {
        return Ok(None);
    }
    let destination = fields
        .next()
        .ok_or_else(|| ChannelError::Protocol("irc target is missing".to_owned()))?;
    if body.is_empty() {
        return Ok(None);
    }
    let (sender, mut metadata) = sender(prefix.unwrap_or("unknown"));
    if let Some(tags) = tags {
        add_tags(&mut metadata, tags);
    }
    let conversation = if destination.starts_with(['#', '&', '+', '!']) {
        destination
    } else {
        sender.id.as_str()
    };
    Ok(Some(InboundMessage {
        id: format!("{}-{:x}", channel, stable_id(line)),
        target: MessageTarget {
            channel,
            conversation: ConversationId::new(conversation)?,
            thread: None,
            reply_to: None,
        },
        sender,
        body: MessageBody::text(body)?,
        timestamp_ms: None,
        metadata,
    }))
}

pub(in crate::platform) fn encode(
    path: &str,
    message: &OutboundMessage,
) -> Result<OutboundRequest, ChannelError> {
    message.body.validate()?;
    if !message.body.attachments.is_empty() {
        return Err(ChannelError::Unsupported("irc attachments".to_owned()));
    }
    Ok(OutboundRequest {
        method: "POST".to_owned(),
        path: path.to_owned(),
        content_type: "text/plain".to_owned(),
        body: format!(
            "PRIVMSG {} :{}\r\n",
            message.target.conversation, message.body.text
        ),
        headers: BTreeMap::new(),
    })
}

fn sender(prefix: &str) -> (Participant, BTreeMap<String, String>) {
    let (nick, rest) = prefix.split_once('!').unwrap_or((prefix, ""));
    let (user, host) = rest.split_once('@').unwrap_or((rest, ""));
    let mut metadata = BTreeMap::new();
    metadata.insert("irc.nick".to_owned(), nick.to_owned());
    if !user.is_empty() {
        metadata.insert("irc.user".to_owned(), user.to_owned());
    }
    if !host.is_empty() {
        metadata.insert("irc.host".to_owned(), host.to_owned());
    }
    (
        Participant {
            id: nick.to_owned(),
            display_name: None,
            handle: Some(nick.to_owned()),
        },
        metadata,
    )
}

fn add_tags(metadata: &mut BTreeMap<String, String>, tags: &str) {
    for tag in tags.split(';') {
        let Some((name, value)) = tag.split_once('=') else {
            continue;
        };
        if !name.is_empty() {
            metadata.insert(format!("irc.tag.{name}"), value.to_owned());
        }
    }
}

fn stable_id(line: &str) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut hash);
    hash.finish()
}
