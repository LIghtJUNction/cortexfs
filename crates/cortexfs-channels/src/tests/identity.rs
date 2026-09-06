use crate::platform::{discord::DiscordCodec, slack::SlackCodec, telegram::TelegramCodec};
use crate::{ChannelCodec, ChannelError};

#[test]
fn telegram_never_uses_chat_or_forwarded_subject_as_sender() -> Result<(), ChannelError> {
    for payload in [
        r#"{"message":{"message_id":1,"chat":{"id":7},"text":"hello"}}"#,
        r#"{"channel_post":{"message_id":1,"chat":{"id":7},"text":"hello"}}"#,
        r#"{"message":{"message_id":1,"chat":{"id":9},"from":{"id":7},"sender_chat":{"id":9},"text":"hello"}}"#,
    ] {
        assert!(TelegramCodec.decode(payload)?.is_none());
    }
    let message = TelegramCodec.decode(
        r#"{"message":{"message_id":1,"chat":{"id":9},"from":{"id":8},"forward_from":{"id":7},"text":"sender=7"}}"#,
    )?;
    assert_eq!(
        message.as_ref().map(|item| item.sender.id.as_str()),
        Some("8")
    );
    Ok(())
}

#[test]
fn telegram_anonymous_events_have_no_user_actor() -> Result<(), ChannelError> {
    for payload in [
        r#"{"edited_message":{"message_id":1,"chat":{"id":9},"from":{"id":7},"sender_chat":{"id":9},"text":"edit"}}"#,
        r#"{"message_reaction":{"chat":{"id":9},"message_id":1,"user":{"id":7},"actor_chat":{"id":9},"new_reaction":[{"emoji":"x"}]}}"#,
    ] {
        let event = TelegramCodec
            .decode_event(payload)?
            .ok_or_else(|| ChannelError::Protocol("missing event".to_owned()))?;
        assert!(event.context().participant.is_none());
    }
    let event = TelegramCodec.decode_event(
        r#"{"edited_message":{"message_id":1,"chat":{"id":9},"from":{"id":8},"forward_from":{"id":7},"text":"edit"}}"#,
    )?.ok_or_else(|| ChannelError::Protocol("missing edit".to_owned()))?;
    assert_eq!(
        event
            .context()
            .participant
            .as_ref()
            .map(|user| user.id.as_str()),
        Some("8")
    );
    Ok(())
}

#[test]
fn discord_events_distinguish_actor_from_message_author() -> Result<(), ChannelError> {
    for kind in [
        "MESSAGE_UPDATE",
        "MESSAGE_DELETE",
        "MESSAGE_REACTION_ADD",
        "MESSAGE_REACTION_REMOVE",
        "TYPING_START",
    ] {
        let payload = serde_json::json!({
            "t": kind,
            "d": {"id": "m", "message_id": "m", "channel_id": "c", "content": "edit",
                "author": {"id": "allowed"}, "user_id": "outsider", "emoji": {"name": "x"}}
        });
        let event = DiscordCodec
            .decode_event(&payload.to_string())?
            .ok_or_else(|| ChannelError::Protocol("missing event".to_owned()))?;
        let expected = match kind {
            "MESSAGE_UPDATE" | "MESSAGE_DELETE" => None,
            _ => Some("outsider"),
        };
        assert_eq!(
            event
                .context()
                .participant
                .as_ref()
                .map(|user| user.id.as_str()),
            expected
        );
    }
    Ok(())
}

#[test]
fn slack_edits_authorize_editor_and_never_fall_back_to_author() -> Result<(), ChannelError> {
    for (edited, expected) in [
        (serde_json::json!({"user": "outsider"}), Some("outsider")),
        (serde_json::Value::Null, None),
    ] {
        let payload = serde_json::json!({"event": {
            "type": "message", "subtype": "message_changed", "channel": "c",
            "message": {"ts": "1.2", "text": "edit", "user": "allowed", "edited": edited}
        }});
        let event = SlackCodec
            .decode_event(&payload.to_string())?
            .ok_or_else(|| ChannelError::Protocol("missing edit".to_owned()))?;
        assert_eq!(
            event
                .context()
                .participant
                .as_ref()
                .map(|user| user.id.as_str()),
            expected
        );
    }
    Ok(())
}

#[test]
fn slack_deletes_never_authorize_the_previous_message_author() -> Result<(), ChannelError> {
    let event = SlackCodec.decode_event(
        r#"{"event":{"type":"message","subtype":"message_deleted","channel":"c","deleted_ts":"1.2","previous_message":{"ts":"1.2","user":"allowed"}}}"#,
    )?.ok_or_else(|| ChannelError::Protocol("missing delete".to_owned()))?;
    assert!(event.context().participant.is_none());
    Ok(())
}
