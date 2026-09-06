use serde_json::Value;

use super::super::{object, scalar};
use crate::{ChannelError, ChannelId, ChannelIncomingEvent, MessageBody};

mod parse;

pub(super) fn decode(
    payload: &str,
    channel: ChannelId,
) -> Result<Option<ChannelIncomingEvent>, ChannelError> {
    let root = object(payload)?;
    let kind = root
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = root.get("data").unwrap_or(&root);
    let post = parse::embedded(data.get("post"));
    let reaction = parse::embedded(data.get("reaction"));
    match kind {
        "reaction_added" => {
            reaction_event(&root, data, post.as_ref(), reaction.as_ref(), channel, true).map(Some)
        }
        "reaction_removed" => reaction_event(
            &root,
            data,
            post.as_ref(),
            reaction.as_ref(),
            channel,
            false,
        )
        .map(Some),
        "post_edited" => edited(&root, data, post.as_ref(), channel).map(Some),
        "post_deleted" => deleted(&root, data, post.as_ref(), channel).map(Some),
        "typing" => {
            parse::context(&root, data, post.as_ref(), Some(data), channel).map(|context| {
                Some(ChannelIncomingEvent::Typing {
                    context,
                    active: true,
                })
            })
        }
        _ => Ok(None),
    }
}

fn reaction_event(
    root: &Value,
    data: &Value,
    post: Option<&Value>,
    reaction: Option<&Value>,
    channel: ChannelId,
    added: bool,
) -> Result<ChannelIncomingEvent, ChannelError> {
    let reaction = reaction
        .ok_or_else(|| ChannelError::Protocol("mattermost reaction is missing".to_owned()))?;
    Ok(ChannelIncomingEvent::Reaction {
        context: parse::context(root, data, post, Some(reaction), channel)?,
        message_id: scalar(reaction.get("post_id"), "reaction.post_id")?,
        emoji: scalar(reaction.get("emoji_name"), "reaction.emoji_name")?,
        added,
    })
}

fn edited(
    root: &Value,
    data: &Value,
    post: Option<&Value>,
    channel: ChannelId,
) -> Result<ChannelIncomingEvent, ChannelError> {
    let post =
        post.ok_or_else(|| ChannelError::Protocol("mattermost edited post is missing".to_owned()))?;
    Ok(ChannelIncomingEvent::MessageEdited {
        context: parse::context(root, data, Some(post), None, channel)?,
        message_id: scalar(post.get("id").or_else(|| data.get("post_id")), "post.id")?,
        body: MessageBody::with_attachments(
            post.get("message")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            super::attachments(post),
        )?,
    })
}

fn deleted(
    root: &Value,
    data: &Value,
    post: Option<&Value>,
    channel: ChannelId,
) -> Result<ChannelIncomingEvent, ChannelError> {
    Ok(ChannelIncomingEvent::MessageDeleted {
        context: parse::context(root, data, post, None, channel)?,
        message_id: scalar(
            post.and_then(|value| value.get("id"))
                .or_else(|| data.get("post_id")),
            "post.id",
        )?,
    })
}
