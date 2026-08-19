use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use base64::Engine as _;
use futures_core::Stream;

use crate::platform::{
    ChannelCodec, bluesky::BlueskyCodec, dingtalk::DingTalkCodec, discord::DiscordCodec,
    email::EmailCodec, feishu::FeishuCodec, gmail::GmailCodec, irc::IrcCodec, lark::LarkCodec,
    line::LineCodec, linq::LinqCodec, matrix::MatrixCodec, mattermost::MattermostCodec,
    mochat::MochatCodec, nextcloud::NextcloudCodec, notion::NotionCodec, qq::QqCodec,
    reddit::RedditCodec, signal::SignalCodec, slack::SlackCodec, teams::TeamsCodec,
    telegram::TelegramCodec, twitch::TwitchCodec, twitter::TwitterCodec, wecom::WeComCodec,
    whatsapp::WhatsAppCodec,
};
use crate::*;

struct OneMessageStream(Option<InboundMessage>);

impl Stream for OneMessageStream {
    type Item = Result<InboundMessage, ChannelError>;

    fn poll_next(mut self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

#[derive(Debug)]
struct TestAdapter {
    id: ChannelId,
    inbound: Option<InboundMessage>,
}

impl ChannelAdapter for TestAdapter {
    fn id(&self) -> ChannelId {
        self.id.clone()
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities::text()
    }

    fn listen(&self) -> Result<ChannelStream, ChannelError> {
        Ok(Box::pin(OneMessageStream(self.inbound.clone())))
    }

    fn send(&self, message: OutboundMessage) -> ChannelFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            Ok(DeliveryReceipt {
                channel: self.id(),
                message_id: "reply-1".to_owned(),
                target: message.target,
                timestamp_ms: None,
            })
        })
    }
}

#[test]
fn channel_ids_are_canonical() -> Result<(), ChannelError> {
    assert!(ChannelId::new("Telegram").is_err());
    assert_eq!(
        ChannelId::new("telegram").map(|id| id.to_string()),
        Ok("telegram".to_owned())
    );
    let instance = ChannelId::new("telegram.primary")?;
    assert_eq!(instance.family(), "telegram");
    assert_eq!(instance.instance(), Some("primary"));
    assert_eq!(ChannelId::from_static("telegram").family(), "telegram");
    assert_eq!(ChannelId::from_static("telegram").instance(), None);
    Ok(())
}

#[test]
#[expect(
    clippy::pattern_type_mismatch,
    reason = "the fixture inspects a borrowed incoming item without cloning it"
)]
fn catalog_exposes_native_and_external_channel_families() -> Result<(), ChannelError> {
    let mut ids = std::collections::BTreeSet::new();
    assert!(CHANNEL_CATALOG.iter().all(|spec| ids.insert(spec.id)));
    assert!(
        CHANNEL_CATALOG
            .iter()
            .any(|spec| spec.id == "telegram" && spec.native)
    );
    assert!(
        CHANNEL_CATALOG
            .iter()
            .any(|spec| spec.id == "bluesky" && spec.native)
    );
    assert_eq!(
        platform::catalog::find("nextcloud_talk").map(|spec| spec.transport),
        Some(ChannelTransport::Webhook)
    );
    assert_eq!(
        platform::catalog::find("telegram.primary").map(|spec| spec.id),
        Some("telegram")
    );
    let configured = ChannelId::new("telegram.primary")?;
    let incoming = TelegramCodec.decode_incoming_for(
        configured,
        r#"{"message":{"message_id":1,"date":2,"text":"hi","chat":{"id":9},"from":{"id":7}}}"#,
    )?;
    assert_eq!(
        incoming.as_ref().and_then(|item| match item {
            ChannelIncoming::Message(message) => Some(message.target.channel.as_str()),
            ChannelIncoming::Event(_) => None,
        }),
        Some("telegram.primary")
    );
    assert!(TelegramCodec.capabilities().long_polling);
    assert!(LineCodec.capabilities().webhook);
    assert!(TelegramCodec.actions().edit);
    assert!(TelegramCodec.actions().supports(ChannelAction::Typing));
    assert!(TelegramCodec.actions().supports(ChannelAction::Reaction));
    assert!(TelegramCodec.actions().supports(ChannelAction::Pin));
    assert!(TelegramCodec.actions().supports(ChannelAction::Redact));
    assert!(SlackCodec.actions().delete);
    assert!(SlackCodec.actions().supports(ChannelAction::Reaction));
    assert!(SlackCodec.actions().supports(ChannelAction::Unpin));
    assert!(!SlackCodec.actions().supports(ChannelAction::Typing));
    assert!(!SlackCodec.capabilities().typing);
    assert!(SlackCodec.capabilities().choices);
    assert!(MatrixCodec.actions().mark_read);
    assert!(MatrixCodec.actions().supports(ChannelAction::Reaction));
    assert!(TelegramCodec.capabilities().send_attachments);
    assert!(DiscordCodec.capabilities().receive_attachments);
    assert!(DiscordCodec.capabilities().send_attachments);
    assert!(SlackCodec.capabilities().send_attachments);
    assert!(SlackCodec.capabilities().commands);
    assert!(LinqCodec.capabilities().send_attachments);
    assert!(MattermostCodec.capabilities().send_attachments);
    assert!(TeamsCodec.capabilities().receive_attachments);
    assert!(platform::catalog::find("twitch").is_some_and(|spec| spec.native));
    assert!(platform::catalog::find("reddit").is_some_and(|spec| spec.native));
    assert!(platform::catalog::find("twitter").is_some_and(|spec| spec.native));
    assert!(platform::catalog::find("wecom").is_some_and(|spec| {
        spec.native && spec.capabilities.send && !spec.capabilities.receive
    }));
    assert!(platform::catalog::find("mochat").is_some_and(|spec| spec.native));
    assert!(
        platform::catalog::find("linq")
            .is_some_and(|spec| { spec.native && spec.capabilities.webhook })
    );
    assert!(
        platform::catalog::find("notion")
            .is_some_and(|spec| { spec.native && spec.capabilities.polling })
    );
    assert!(platform::catalog::find("voice_wake").is_some_and(|spec| {
        !spec.native && spec.capabilities.audio && !spec.capabilities.send
    }));
    Ok(())
}

#[test]
fn registry_rejects_duplicate_adapter() -> Result<(), ChannelError> {
    let id = ChannelId::new("test")?;
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(TestAdapter {
        id: id.clone(),
        inbound: None,
    }))?;
    assert_eq!(
        registry.register(Arc::new(TestAdapter {
            id: id.clone(),
            inbound: None,
        })),
        Err(ChannelError::DuplicateChannel("test".to_owned()))
    );
    assert_eq!(registry.ids(), vec![id]);
    Ok(())
}

#[test]
fn registry_unifies_message_and_event_receive_paths() -> Result<(), ChannelError> {
    let id = ChannelId::from_static("test");
    let message = InboundMessage {
        id: "message-1".to_owned(),
        target: MessageTarget {
            channel: id.clone(),
            conversation: ConversationId::new("conversation")?,
            thread: None,
            reply_to: None,
        },
        sender: Participant::default(),
        body: MessageBody::text("hello")?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    };
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(TestAdapter {
        id: id.clone(),
        inbound: Some(message.clone()),
    }))?;
    let mut stream = registry.receive_incoming(&id)?;
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let item = match Stream::poll_next(Pin::as_mut(&mut stream), &mut context) {
        Poll::Ready(Some(item)) => item?,
        Poll::Ready(None) | Poll::Pending => {
            return Err(ChannelError::Protocol(
                "message stream did not yield its item".to_owned(),
            ));
        }
    };
    assert_eq!(item, ChannelIncoming::Message(message));
    Ok(())
}

#[test]
fn empty_message_body_is_rejected() {
    assert!(MessageBody::default().validate().is_err());
}

#[test]
fn capabilities_keep_old_wire_frames_compatible() -> Result<(), ChannelError> {
    let capabilities: ChannelCapabilities = serde_json::from_str(
        r#"{"receive":true,"send":true,"threads":false,"media":false,"reactions":false,"typing":false,"webhook":false}"#,
    )
    .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    assert!(capabilities.receive);
    assert!(!capabilities.attachments);
    assert!(!capabilities.receive_attachments);
    let frame = ChannelFrame::decode(
        br#"{"abi":"cortexfs.channel.socket/v1","frame":{"type":"hello","request_id":"r","channel":"telegram","capabilities":{"receive":true,"send":true}}}
"#,
    )
    .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    assert!(
        matches!(frame.frame, ChannelFrameBody::Hello { actions, .. } if actions == ChannelActions::empty())
    );
    Ok(())
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the codec matrix intentionally keeps one cross-platform ABI fixture together"
)]
fn platform_codecs_normalize_native_payloads() -> Result<(), ChannelError> {
    let telegram = TelegramCodec.decode(
        r#"{"update_id":1,"message":{"message_id":2,"date":3,"text":"hi","chat":{"id":9},"from":{"id":7}}}"#,
    )?;
    assert_eq!(
        telegram
            .as_ref()
            .map(|message| message.target.conversation.as_str()),
        Some("9")
    );
    assert_eq!(
        telegram.as_ref().map(|message| message.body.text.as_str()),
        Some("hi")
    );

    let discord = DiscordCodec.decode(
        r#"{"id":"m","channel_id":"c","content":"hello","author":{"id":"u","username":"user"}}"#,
    )?;
    assert_eq!(
        discord
            .as_ref()
            .map(|message| message.target.channel.as_str()),
        Some("discord")
    );

    let slack = SlackCodec.decode(
        r#"{"event":{"type":"message","ts":"1.2","channel":"C1","user":"U1","text":"hello"}}"#,
    )?;
    assert_eq!(
        slack
            .as_ref()
            .map(|message| message.target.conversation.as_str()),
        Some("C1")
    );
    assert_eq!(
        SlackCodec.challenge(r#"{"type":"url_verification","challenge":"x"}"#),
        Some("x".to_owned())
    );

    let feishu = FeishuCodec.decode(
        r#"{"event":{"message":{"message_id":"m","chat_id":"c","content":"{\"text\":\"hello\"}"},"sender":{"sender_id":{"open_id":"u"}}}}"#,
    )?;
    assert_eq!(
        feishu.as_ref().map(|message| message.body.text.as_str()),
        Some("hello")
    );
    let lark = LarkCodec.decode(
        r#"{"event":{"message":{"message_id":"m","chat_id":"c","content":"{\"text\":\"hello\"}"},"sender":{"sender_id":{"open_id":"u"}}}}"#,
    )?;
    assert_eq!(
        lark.as_ref().map(|message| message.target.channel.as_str()),
        Some("lark")
    );

    let dingtalk = DingTalkCodec.decode(
        r#"{"type":"CALLBACK","headers":{"messageId":"m"},"data":"{\"text\":{\"content\":\"hello\"},\"senderStaffId\":\"u\",\"conversationType\":2,\"conversationId\":\"c\",\"sessionWebhook\":\"https://example.invalid/h\"}"}"#,
    )?;
    assert_eq!(
        dingtalk
            .as_ref()
            .map(|message| message.target.conversation.as_str()),
        Some("c")
    );
    assert_eq!(
        DingTalkCodec::session_webhook(
            r#"{"data":{"sessionWebhook":"https://example.invalid/h"}}"#
        ),
        Some("https://example.invalid/h".to_owned())
    );

    let matrix = MatrixCodec.decode(
        r#"{"type":"m.room.message","event_id":"$m","room_id":"!room:example.org","sender":"@u:example.org","origin_server_ts":7,"content":{"msgtype":"m.text","body":"hello","m.relates_to":{"m.in_reply_to":{"event_id":"$root"}}}}"#,
    )?;
    assert_eq!(
        matrix
            .as_ref()
            .and_then(|message| message.target.reply_to.as_deref()),
        Some("$root")
    );
    let outbound = MatrixCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("matrix")?,
            conversation: ConversationId::new("!room:example.org")?,
            thread: Some("$thread".to_owned()),
            reply_to: Some("$reply".to_owned()),
        },
        body: MessageBody::text("answer")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(outbound.body.contains("m.in_reply_to"));

    let whatsapp = WhatsAppCodec.decode(
        r#"{"entry":[{"changes":[{"value":{"messages":[{"from":"8613800138000","id":"wamid-1","timestamp":"7","type":"text","text":{"body":"hello"}}]}}]}]}"#,
    )?;
    assert_eq!(
        whatsapp.as_ref().map(|message| message.sender.id.as_str()),
        Some("+8613800138000")
    );
    let outbound = WhatsAppCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("whatsapp")?,
            conversation: ConversationId::new("+8613800138000")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::text("answer")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(outbound.body.contains("8613800138000"));
    let twitter = TwitterCodec.decode(
        r#"{"data":[{"id":"t1","text":"hello","author_id":"u1","conversation_id":"c1","created_at":"2026-08-17T10:00:00Z"}],"includes":{"users":[{"id":"u1","username":"alice","name":"Alice"}]}}"#,
    )?;
    let twitter =
        twitter.ok_or_else(|| ChannelError::Protocol("missing Twitter message".to_owned()))?;
    assert_eq!(twitter.sender.handle.as_deref(), Some("alice"));
    let reply = TwitterCodec.encode(&OutboundMessage {
        target: twitter.target,
        body: MessageBody::text("answer")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(reply.body.contains("in_reply_to_tweet_id"));
    let dm = TwitterCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("twitter")?,
            conversation: ConversationId::new("u2")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::text("private")?,
        metadata: std::iter::once(("twitter.dm_recipient".to_owned(), "u2".to_owned())).collect(),
    })?;
    assert!(dm.path.contains("dm_conversations/with/u2"));
    let wecom = WeComCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("wecom")?,
            conversation: ConversationId::new("broadcast")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::text("announcement")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(wecom.body.contains("announcement"));
    let mochat = MochatCodec.decode(
        r#"{"data":[{"messageId":"m1","fromUserId":"u1","chatId":"c1","content":{"text":"hello"},"timestamp":"7"}]}"#,
    )?;
    let mochat =
        mochat.ok_or_else(|| ChannelError::Protocol("missing Mochat message".to_owned()))?;
    assert_eq!(mochat.target.conversation.as_str(), "c1");
    let mochat_reply = MochatCodec.encode(&OutboundMessage {
        target: mochat.target,
        body: MessageBody::text("answer")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert_eq!(mochat_reply.path, "api/message/send");
    let linq = LinqCodec.decode(
        r#"{"event_type":"message.received","event_id":"evt-1","created_at":"2026-08-17T10:00:00Z","data":{"chat_id":"chat-1","from":"13800138000","message":{"id":"m1","parts":[{"type":"text","value":"hello"},{"type":"image","url":"https://example.invalid/a.png","mime_type":"image/png"}]}}}"#,
    )?;
    let linq = linq.ok_or_else(|| ChannelError::Protocol("missing Linq message".to_owned()))?;
    assert_eq!(linq.sender.id, "+13800138000");
    assert_eq!(linq.body.attachments.len(), 1);
    let linq_reply = LinqCodec.encode(&OutboundMessage {
        target: linq.target,
        body: MessageBody::with_attachments(
            "answer",
            vec![Attachment {
                url: "https://example.invalid/out.png".to_owned(),
                name: Some("out.png".to_owned()),
                mime: Some("image/png".to_owned()),
            }],
        )?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert_eq!(linq_reply.path, "chats/chat-1/messages");
    let linq_body: serde_json::Value = serde_json::from_str(&linq_reply.body)
        .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    let parts = linq_body
        .get("message")
        .and_then(|message| message.get("parts"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ChannelError::Protocol("missing Linq parts".to_owned()))?;
    let media = parts
        .get(1)
        .ok_or_else(|| ChannelError::Protocol("missing Linq media part".to_owned()))?;
    assert_eq!(
        media.get("type").and_then(serde_json::Value::as_str),
        Some("media")
    );
    assert_eq!(
        media.get("url").and_then(serde_json::Value::as_str),
        Some("https://example.invalid/out.png")
    );
    let notion = NotionCodec::default().decode(
        r#"{"id":"page-1","created_by":{"id":"user-1"},"properties":{"Status":{"type":"select","select":{"name":"pending"}},"Input":{"type":"rich_text","rich_text":[{"plain_text":"task"}]}}}"#,
    )?;
    let notion = notion.ok_or_else(|| ChannelError::Protocol("missing Notion page".to_owned()))?;
    assert_eq!(notion.sender.id, "user-1");
    let notion_reply = NotionCodec::default().encode(&OutboundMessage {
        target: notion.target,
        body: MessageBody::text("done")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert_eq!(notion_reply.path, "pages/page-1");
    let whatsapp_batch = WhatsAppCodec.decode_many(
        r#"{"entry":[{"changes":[{"value":{"messages":[{"from":"8613800138000","id":"m1","type":"text","text":{"body":"one"}},{"from":"8613800138001","id":"m2","type":"text","text":{"body":"two"}}]}}]}]}"#,
    )?;
    assert_eq!(whatsapp_batch.len(), 2);

    let push_data = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(r#"{"emailAddress":"bot@example.com","historyId":"42"}"#);
    let cursor = GmailCodec::push_cursor(&format!(r#"{{"message":{{"data":"{push_data}"}}}}"#))?;
    assert_eq!(
        cursor.map(|cursor| cursor.history_id),
        Some("42".to_owned())
    );
    Ok(())
}

#[test]
fn telegram_codec_emits_provider_neutral_events() -> Result<(), ChannelError> {
    let incoming = TelegramCodec.decode_incoming(
        r#"{"message_reaction":{"chat":{"id":9},"message_id":2,"user":{"id":7},"new_reaction":[{"type":"emoji","emoji":"👍"}],"old_reaction":[]}}"#,
    )?;
    let Some(ChannelIncoming::Event(ChannelIncomingEvent::Reaction {
        context,
        message_id,
        emoji,
        added,
    })) = incoming
    else {
        return Err(ChannelError::Protocol(
            "telegram reaction was not normalized".to_owned(),
        ));
    };
    assert_eq!(context.target.channel.as_str(), "telegram");
    assert_eq!(context.target.conversation.as_str(), "9");
    assert_eq!(
        context.participant.as_ref().map(|item| item.id.as_str()),
        Some("7")
    );
    assert_eq!(message_id, "2");
    assert_eq!(emoji, "👍");
    assert!(added);
    Ok(())
}

#[test]
fn matrix_codec_emits_provider_neutral_events() -> Result<(), ChannelError> {
    let reaction = MatrixCodec.decode_incoming(
        r#"{"type":"m.reaction","event_id":"$r","room_id":"!room:example.org","sender":"@u:example.org","origin_server_ts":8,"content":{"m.relates_to":{"rel_type":"m.annotation","event_id":"$m","key":"👍"}}}"#,
    )?;
    assert!(matches!(
        reaction,
        Some(ChannelIncoming::Event(ChannelIncomingEvent::Reaction {
            message_id,
            emoji,
            added: true,
            ..
        })) if message_id == "$m" && emoji == "👍"
    ));
    let edited = MatrixCodec.decode_incoming(
        r#"{"type":"m.room.message","event_id":"$e","room_id":"!room:example.org","sender":"@u:example.org","content":{"m.new_content":{"msgtype":"m.text","body":"new"},"m.relates_to":{"rel_type":"m.replace","event_id":"$m"}}}"#,
    )?;
    assert!(matches!(
        edited,
        Some(ChannelIncoming::Event(ChannelIncomingEvent::MessageEdited {
            message_id,
            body,
            ..
        })) if message_id == "$m" && body.text == "new"
    ));
    let deleted = MatrixCodec.decode_incoming(
        r#"{"type":"m.room.redaction","event_id":"$r","room_id":"!room:example.org","sender":"@u:example.org","redacts":"$m","content":{}}"#,
    )?;
    assert!(matches!(
        deleted,
        Some(ChannelIncoming::Event(ChannelIncomingEvent::MessageDeleted {
            message_id,
            ..
        })) if message_id == "$m"
    ));
    Ok(())
}

#[test]
fn discord_and_slack_codecs_emit_common_events() -> Result<(), ChannelError> {
    let discord = DiscordCodec.decode_incoming(
        r#"{"t":"MESSAGE_REACTION_ADD","d":{"channel_id":"c","message_id":"m","user_id":"u","emoji":{"name":"👍"}}}"#,
    )?;
    assert!(matches!(
        discord,
        Some(ChannelIncoming::Event(ChannelIncomingEvent::Reaction {
            message_id,
            emoji,
            added: true,
            ..
        })) if message_id == "m" && emoji == "👍"
    ));
    let slack = SlackCodec.decode_incoming(
        r#"{"event":{"type":"reaction_added","user":"U1","reaction":"thumbsup","item":{"channel":"C1","ts":"1.2"}}}"#,
    )?;
    assert!(matches!(
        slack,
        Some(ChannelIncoming::Event(ChannelIncomingEvent::Reaction {
            message_id,
            emoji,
            added: true,
            ..
        })) if message_id == "1.2" && emoji == "thumbsup"
    ));
    let edited = SlackCodec.decode_incoming(
        r#"{"event":{"type":"message_changed","message":{"channel":"C1","ts":"1.2","user":"U1","text":"edited"}}}"#,
    )?;
    assert!(matches!(
        edited,
        Some(ChannelIncoming::Event(ChannelIncomingEvent::MessageEdited {
            message_id,
            body,
            ..
        })) if message_id == "1.2" && body.text == "edited"
    ));
    let edited_outer_channel = SlackCodec.decode_incoming(
        r#"{"event":{"type":"message_changed","channel":"C1","message":{"ts":"1.3","user":"U1","text":"outer"}}}"#,
    )?;
    assert!(matches!(
        edited_outer_channel,
        Some(ChannelIncoming::Event(ChannelIncomingEvent::MessageEdited { context, .. }))
            if context.target.conversation.as_str() == "C1"
    ));
    let deleted_outer_channel = SlackCodec.decode_incoming(
        r#"{"event":{"type":"message_deleted","channel":"C1","previous_message":{"ts":"1.4","user":"U1"},"deleted_ts":"1.4"}}"#,
    )?;
    assert!(matches!(
        deleted_outer_channel,
        Some(ChannelIncoming::Event(ChannelIncomingEvent::MessageDeleted { context, .. }))
            if context.target.conversation.as_str() == "C1"
    ));
    let mention = SlackCodec.decode(
        r#"{"event":{"type":"app_mention","ts":"2.3","channel":"C1","user":"U1","text":"hello"}}"#,
    )?;
    assert!(mention.is_some());
    Ok(())
}

#[test]
fn line_and_teams_codecs_preserve_reply_targets() -> Result<(), ChannelError> {
    let line = LineCodec.decode(
        r#"{"events":[{"type":"message","timestamp":7,"replyToken":"reply-1","source":{"type":"user","userId":"U1"},"message":{"type":"text","id":"m1","text":"hello"}}]}"#,
    )?;
    assert_eq!(
        line.as_ref().map(|message| message.sender.id.as_str()),
        Some("U1")
    );
    let line_reply = LineCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("line")?,
            conversation: ConversationId::new("U1")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::text("answer")?,
        metadata: std::iter::once(("line.reply_token".to_owned(), "reply-1".to_owned())).collect(),
    })?;
    assert_eq!(line_reply.path, "v2/bot/message/reply");
    let line_batch = LineCodec.decode_many(
        r#"{"events":[{"type":"message","source":{"type":"user","userId":"U1"},"message":{"type":"text","id":"m1","text":"one"}},{"type":"message","source":{"type":"user","userId":"U2"},"message":{"type":"text","id":"m2","text":"two"}}]}"#,
    )?;
    assert_eq!(line_batch.len(), 2);
    let line_batch_incoming = LineCodec.decode_many_incoming(
        r#"{"events":[{"type":"message","source":{"type":"user","userId":"U1"},"message":{"type":"text","id":"m1","text":"one"}},{"type":"message","source":{"type":"user","userId":"U2"},"message":{"type":"text","id":"m2","text":"two"}}]}"#,
    )?;
    assert_eq!(line_batch_incoming.len(), 2);

    let teams = TeamsCodec.decode(
        r#"{"type":"message","id":"a1","timestamp":"7","serviceUrl":"https://teams.example","channelId":"msteams","from":{"id":"u1","name":"User"},"conversation":{"id":"c1"},"text":"hello"}"#,
    )?;
    assert_eq!(
        teams
            .as_ref()
            .map(|message| message.target.conversation.as_str()),
        Some("c1")
    );
    let teams_reply = TeamsCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("teams")?,
            conversation: ConversationId::new("c1")?,
            thread: None,
            reply_to: Some("a1".to_owned()),
        },
        body: MessageBody::text("answer")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(teams_reply.path.contains("c1"));
    let teams_media = TeamsCodec.decode(
        r#"{"type":"message","id":"a2","from":{"id":"u1"},"conversation":{"id":"c1"},"attachments":[{"contentType":"image/png","contentUrl":"https://files.example/teams.png","name":"teams.png"}]}"#,
    )?;
    assert_eq!(
        teams_media
            .as_ref()
            .map(|message| message.body.attachments.len()),
        Some(1)
    );
    let teams_attachment = TeamsCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("teams")?,
            conversation: ConversationId::new("c1")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::with_attachments(
            "image",
            vec![Attachment {
                url: "https://files.example/teams.png".to_owned(),
                name: Some("teams.png".to_owned()),
                mime: Some("image/png".to_owned()),
            }],
        )?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(teams_attachment.body.contains("contentUrl"));

    let nextcloud = NextcloudCodec.decode(
        r#"{"type":"create","target":{"id":"room-1"},"actor":{"type":"Person","id":"users/alice"},"object":{"type":"Note","id":"note-1","content":"{\"message\":\"hello\"}"}}"#,
    )?;
    assert_eq!(
        nextcloud
            .as_ref()
            .map(|message| message.target.conversation.as_str()),
        Some("room-1")
    );
    let nextcloud_reply = NextcloudCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("nextcloud_talk")?,
            conversation: ConversationId::new("room-1")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::text("answer")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(nextcloud_reply.path.contains("room-1"));
    Ok(())
}

#[test]
fn bluesky_codec_normalizes_mentions_and_reply_records() -> Result<(), ChannelError> {
    let inbound = BlueskyCodec.decode(
        r#"{"notifications":[{"reason":"mention","isRead":false,"uri":"at://did:plc:u/app.bsky.feed.post/p","cid":"bafy-p","indexedAt":"2026-08-17T10:00:00Z","author":{"did":"did:plc:u","handle":"alice.test"},"record":{"text":"hello"}}]}"#,
    )?;
    let inbound =
        inbound.ok_or_else(|| ChannelError::Protocol("missing Bluesky message".to_owned()))?;
    assert_eq!(inbound.sender.id, "did:plc:u");
    assert_eq!(
        inbound.metadata.get("bluesky.reply_cid"),
        Some(&"bafy-p".to_owned())
    );
    let mut metadata = inbound.metadata;
    metadata.insert("bluesky.repo".to_owned(), "did:plc:bot".to_owned());
    metadata.insert(
        "bluesky.created_at".to_owned(),
        "2026-08-17T10:00:01Z".to_owned(),
    );
    let outbound = BlueskyCodec.encode(&OutboundMessage {
        target: inbound.target,
        body: MessageBody::text("answer")?,
        metadata,
    })?;
    assert!(outbound.body.contains("did:plc:bot"));
    assert!(outbound.body.contains("bafy-p"));
    Ok(())
}

#[test]
fn platform_attachments_stay_in_the_generic_body() -> Result<(), ChannelError> {
    let discord = DiscordCodec.decode(
        r#"{"id":"m","channel_id":"c","content":"look","author":{"id":"u"},"attachments":[{"url":"https://files.example/a.png","filename":"a.png","content_type":"image/png"}]}"#,
    )?;
    assert_eq!(
        discord
            .as_ref()
            .map(|message| message.body.attachments.len()),
        Some(1)
    );
    let slack = SlackCodec.decode(
        r#"{"event":{"type":"message","ts":"1.2","channel":"C1","user":"U1","files":[{"url_private":"https://files.example/a","name":"a","mimetype":"image/png"}]}}"#,
    )?;
    assert_eq!(
        slack
            .as_ref()
            .and_then(|message| message.body.attachments.first())
            .and_then(|attachment| attachment.mime.as_deref()),
        Some("image/png")
    );
    let body = MessageBody::with_attachments(
        "caption",
        vec![Attachment {
            url: "https://files.example/a.png".to_owned(),
            name: Some("a.png".to_owned()),
            mime: Some("image/png".to_owned()),
        }],
    )?;
    let telegram = TelegramCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("telegram")?,
            conversation: ConversationId::new("c")?,
            thread: None,
            reply_to: None,
        },
        body: body.clone(),
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert_eq!(telegram.path, "sendPhoto");
    let whatsapp = WhatsAppCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("whatsapp")?,
            conversation: ConversationId::new("+8613800138000")?,
            thread: None,
            reply_to: None,
        },
        body: body.clone(),
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(whatsapp.body.contains("\"type\":\"image\""));
    let discord = DiscordCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("discord")?,
            conversation: ConversationId::new("c")?,
            thread: None,
            reply_to: None,
        },
        body: body.clone(),
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(discord.body.contains("\"embeds\":[{"));
    assert!(
        discord
            .body
            .contains("\"image\":{\"url\":\"https://files.example/a.png\"}")
    );
    let slack = SlackCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("slack")?,
            conversation: ConversationId::new("C1")?,
            thread: Some("1.2".to_owned()),
            reply_to: None,
        },
        body,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(slack.body.contains("\"blocks\":[{"));
    assert!(
        slack
            .body
            .contains("\"image_url\":\"https://files.example/a.png\"")
    );
    Ok(())
}

#[test]
fn additional_platform_codecs_preserve_native_targets() -> Result<(), ChannelError> {
    let email = EmailCodec
        .decode("From: user@example.org\r\nSubject: hello\r\nMessage-ID: <m1>\r\n\r\nbody")?;
    assert_eq!(
        email.as_ref().map(|message| message.sender.id.as_str()),
        Some("user@example.org")
    );
    let email_reply = EmailCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("email")?,
            conversation: ConversationId::new("<m1>")?,
            thread: None,
            reply_to: Some("<m1>".to_owned()),
        },
        body: MessageBody::text("answer")?,
        metadata: std::iter::once(("email.from".to_owned(), "user@example.org".to_owned()))
            .collect(),
    })?;
    assert!(email_reply.body.contains("In-Reply-To: <m1>"));

    let irc = IrcCodec.decode(":nick!user@example.org PRIVMSG #c :hello\r\n")?;
    assert_eq!(
        irc.as_ref()
            .map(|message| message.target.conversation.as_str()),
        Some("#c")
    );
    let irc_reply = IrcCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("irc")?,
            conversation: ConversationId::new("#c")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::text("answer")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert_eq!(irc_reply.body, "PRIVMSG #c :answer\r\n");

    let twitch = TwitchCodec
        .decode("@display-name=Viewer;badges= :viewer!user@host PRIVMSG #stream :@bot hello\r\n")?;
    assert_eq!(
        twitch
            .as_ref()
            .map(|message| message.target.channel.as_str()),
        Some("twitch")
    );
    assert_eq!(
        twitch
            .as_ref()
            .and_then(|message| message.metadata.get("irc.tag.display-name"))
            .map(String::as_str),
        Some("Viewer")
    );
    assert_eq!(
        platform::twitch::normalize_oauth_token(" token "),
        "oauth:token"
    );
    assert_eq!(
        platform::twitch::normalize_channel(" #Stream ").as_deref(),
        Some("#stream")
    );

    let reddit = RedditCodec.decode_many(
        r#"{"data":{"children":[{"data":{"name":"t1_reply","author":"alice","body":"hello","parent_id":"t1_parent","subreddit":"rust","created_utc":1700000000.5,"new":true,"type":"comment_reply"}}]}}"#,
    )?;
    let reddit = reddit
        .into_iter()
        .next()
        .ok_or_else(|| ChannelError::Protocol("missing Reddit message".to_owned()))?;
    assert_eq!(reddit.target.conversation.as_str(), "t1_parent");
    assert_eq!(
        reddit.metadata.get("reddit.kind").map(String::as_str),
        Some("comment")
    );
    let request = RedditCodec.encode(&OutboundMessage {
        target: reddit.target,
        body: MessageBody::text("thanks & bye")?,
        metadata: reddit.metadata,
    })?;
    assert_eq!(request.path, "api/comment");
    assert!(request.body.contains("thanks%20%26%20bye"));

    let signal = SignalCodec.decode(
        r#"{"envelope":{"source":"+8613800138000","sourceName":"user","timestamp":7,"dataMessage":{"message":"hello","groupInfo":{"groupId":"group-1"}}}}"#,
    )?;
    assert_eq!(
        signal
            .as_ref()
            .map(|message| message.target.conversation.as_str()),
        Some("group-1")
    );
    let signal_reply = SignalCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("signal")?,
            conversation: ConversationId::new("+8613800138000")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::text("answer")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(signal_reply.body.contains("\"method\":\"send\""));
    Ok(())
}

#[test]
fn mattermost_codec_unwraps_posted_event_and_threads_reply() -> Result<(), ChannelError> {
    let post = serde_json::json!({
        "id": "post-1",
        "channel_id": "channel-1",
        "user_id": "user-1",
        "root_id": "root-1",
        "message": "hello",
        "props": {"attachments": [{"image_url":"https://files.example/mm.png","title":"mm.png"}]}
    });
    let payload = serde_json::json!({
        "event": "posted",
        "data": {"post": post.to_string()}
    });
    let inbound = MattermostCodec
        .decode(&payload.to_string())?
        .ok_or_else(|| ChannelError::Protocol("missing Mattermost message".to_owned()))?;
    assert_eq!(inbound.target.conversation.as_str(), "channel-1");
    assert_eq!(inbound.target.thread.as_deref(), Some("root-1"));
    assert_eq!(inbound.body.attachments.len(), 1);
    let outbound = MattermostCodec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: inbound.target.channel,
            conversation: inbound.target.conversation,
            thread: inbound.target.thread,
            reply_to: None,
        },
        body: MessageBody::with_attachments(
            "reply",
            vec![Attachment {
                url: "https://files.example/reply.png".to_owned(),
                name: Some("reply.png".to_owned()),
                mime: Some("image/png".to_owned()),
            }],
        )?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    assert!(outbound.body.contains("root-1"));
    assert!(outbound.body.contains("image_url"));
    Ok(())
}

#[test]
fn mattermost_codec_normalizes_events_and_effects() -> Result<(), ChannelError> {
    let post = serde_json::json!({
        "id": "post-1",
        "channel_id": "channel-1",
        "user_id": "author-1",
        "root_id": "root-1",
        "message": "edited"
    });
    let reaction = serde_json::json!({
        "user_id": "user-1",
        "post_id": "post-1",
        "emoji_name": "thumbsup",
        "create_at": 7
    });
    let payload = serde_json::json!({
        "event": "reaction_added",
        "data": {"reaction": reaction.to_string(), "post": post.to_string()}
    });
    let Some(ChannelIncoming::Event(ChannelIncomingEvent::Reaction {
        context,
        message_id,
        emoji,
        added,
    })) = MattermostCodec.decode_incoming(&payload.to_string())?
    else {
        return Err(ChannelError::Protocol(
            "missing Mattermost reaction".to_owned(),
        ));
    };
    assert_eq!(context.target.conversation.as_str(), "channel-1");
    assert_eq!(context.target.thread.as_deref(), Some("root-1"));
    assert_eq!(message_id, "post-1");
    assert_eq!(emoji, "thumbsup");
    assert!(added);

    let edit = serde_json::json!({
        "event": "post_edited",
        "data": {"post": post.to_string()}
    });
    assert!(matches!(
        MattermostCodec.decode_incoming(&edit.to_string())?,
        Some(ChannelIncoming::Event(
            ChannelIncomingEvent::MessageEdited { .. }
        ))
    ));
    let target = MessageTarget {
        channel: ChannelId::from_static("mattermost"),
        conversation: ConversationId::new("channel-1")?,
        thread: None,
        reply_to: None,
    };
    let request = MattermostCodec.encode_effect(
        &target,
        &ChannelEffect::Reaction {
            message_id: "post-1".to_owned(),
            emoji: "thumbsup".to_owned(),
            remove: false,
        },
    )?;
    let request =
        request.ok_or_else(|| ChannelError::Protocol("missing reaction request".to_owned()))?;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "api/v4/reactions");
    assert!(request.body.contains("post-1"));
    Ok(())
}

#[test]
fn qq_codec_preserves_c2c_group_and_guild_targets() -> Result<(), ChannelError> {
    for (event, field, kind) in [
        ("DIRECT_MESSAGE_CREATE", "openid", "c2c"),
        ("GROUP_AT_MESSAGE_CREATE", "group_openid", "group"),
        ("AT_MESSAGE_CREATE", "channel_id", "guild"),
    ] {
        let payload = serde_json::json!({
            "t": event,
            "id": "message-1",
            field: "target-1",
            "author": {"id": "user-1"},
            "content": "hello"
        });
        let inbound = QqCodec
            .decode(&payload.to_string())?
            .ok_or_else(|| ChannelError::Protocol("missing QQ message".to_owned()))?;
        assert_eq!(
            inbound.metadata.get("qq.target_kind").map(String::as_str),
            Some(kind)
        );
        let outbound = QqCodec.encode(&OutboundMessage {
            target: inbound.target,
            body: MessageBody::text("reply")?,
            metadata: inbound.metadata,
        })?;
        assert!(outbound.path.contains("target-1"));
    }
    let bot = serde_json::json!({
        "t": "AT_MESSAGE_CREATE",
        "id": "message-2",
        "channel_id": "target-1",
        "author": {"id": "bot-1", "bot": true},
        "content": "loop"
    });
    assert!(QqCodec.decode(&bot.to_string())?.is_none());
    Ok(())
}

#[test]
fn route_keeps_threads_and_request_ids_stable() -> Result<(), ChannelError> {
    let route = ChannelSessionRoute::new("coder", "im")?;
    let target = MessageTarget {
        channel: ChannelId::new("slack")?,
        conversation: ConversationId::new("C1")?,
        thread: Some("T1".to_owned()),
        reply_to: None,
    };
    let message = InboundMessage {
        id: "M1".to_owned(),
        target: target.clone(),
        sender: Participant::default(),
        body: MessageBody::text("hello")?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    };
    assert_eq!(route.session_for(&target), route.session_for(&target));
    assert_eq!(
        route.request_id_for(&message),
        route.request_id_for(&message)
    );
    assert_ne!(
        route.session_for(&target),
        route.session_for(&MessageTarget {
            thread: None,
            ..target
        })
    );
    let isolated = route.with_identity_isolation();
    let first = InboundMessage {
        sender: Participant {
            id: "user-a".to_owned(),
            ..Participant::default()
        },
        ..message.clone()
    };
    let second = InboundMessage {
        sender: Participant {
            id: "user-b".to_owned(),
            ..Participant::default()
        },
        ..message
    };
    assert_ne!(
        isolated.session_for_message(&first),
        isolated.session_for_message(&second)
    );
    let event = |id: &str| -> Result<ChannelIncomingEvent, ChannelError> {
        Ok(ChannelIncomingEvent::Typing {
            context: ChannelEventContext {
                target: MessageTarget {
                    channel: ChannelId::new("slack")?,
                    conversation: ConversationId::new("C1")?,
                    thread: Some("T1".to_owned()),
                    reply_to: None,
                },
                participant: Some(Participant {
                    id: id.to_owned(),
                    ..Participant::default()
                }),
                timestamp_ms: None,
                metadata: std::collections::BTreeMap::new(),
            },
            active: true,
        })
    };
    let first_event = event("user-a")?;
    let second_event = event("user-b")?;
    assert_ne!(
        isolated.session_for_event(&first_event),
        isolated.session_for_event(&second_event)
    );
    let repeat_event = event("user-a")?;
    assert_eq!(
        isolated.request_id_for_event(&first_event),
        isolated.request_id_for_event(&repeat_event)
    );
    Ok(())
}

#[test]
fn route_isolates_channel_instances() -> Result<(), ChannelError> {
    let route = ChannelSessionRoute::new("coder", "im")?;
    let conversation = ConversationId::new("chat-1")?;
    let primary = MessageTarget {
        channel: ChannelId::new("telegram.primary")?,
        conversation: conversation.clone(),
        thread: None,
        reply_to: None,
    };
    let secondary = MessageTarget {
        channel: ChannelId::new("telegram.secondary")?,
        conversation,
        thread: None,
        reply_to: None,
    };
    assert_ne!(route.session_for(&primary), route.session_for(&secondary));
    assert_eq!(route.session_for(&primary), route.session_for(&primary));
    Ok(())
}

#[test]
fn channel_socket_frame_round_trips_effect_and_correlation() -> Result<(), ChannelError> {
    let channel = ChannelId::new("telegram")?;
    let conversation = ConversationId::new("chat")?;
    let frame = ChannelFrame::new(ChannelFrameBody::Effect {
        request_id: "effect-1".to_owned(),
        target: MessageTarget {
            channel,
            conversation,
            thread: None,
            reply_to: None,
        },
        effect: ChannelEffect::Preview {
            text: "partial response".to_owned(),
        },
    });
    let encoded = frame
        .encode()
        .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    let decoded = ChannelFrame::decode(&encoded)
        .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    assert_eq!(decoded, frame);
    Ok(())
}

#[test]
fn channel_socket_frame_round_trips_provider_neutral_event() -> Result<(), ChannelError> {
    let target = MessageTarget {
        channel: ChannelId::new("telegram")?,
        conversation: ConversationId::new("chat")?,
        thread: None,
        reply_to: None,
    };
    let frame = ChannelFrame::new(ChannelFrameBody::InboundEvent {
        event_id: "reaction-1".to_owned(),
        event: ChannelIncomingEvent::Reaction {
            context: ChannelEventContext {
                target,
                participant: Some(Participant {
                    id: "user".to_owned(),
                    ..Participant::default()
                }),
                timestamp_ms: Some(1),
                metadata: std::collections::BTreeMap::new(),
            },
            message_id: "message-1".to_owned(),
            emoji: "👍".to_owned(),
            added: true,
        },
    });
    let encoded = frame
        .encode()
        .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    let decoded = ChannelFrame::decode(&encoded)
        .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    assert_eq!(decoded, frame);
    Ok(())
}

#[test]
fn platform_codecs_encode_common_effects_without_provider_types() -> Result<(), ChannelError> {
    let target = MessageTarget {
        channel: ChannelId::new("telegram")?,
        conversation: ConversationId::new("chat")?,
        thread: None,
        reply_to: None,
    };
    let reaction = TelegramCodec
        .encode_effect(
            &target,
            &ChannelEffect::Reaction {
                message_id: "message".to_owned(),
                emoji: "👍".to_owned(),
                remove: false,
            },
        )?
        .ok_or_else(|| ChannelError::Protocol("telegram effect missing".to_owned()))?;
    assert_eq!(reaction.path, "setMessageReaction");
    assert!(reaction.body.contains("message"));

    let pin = TelegramCodec
        .encode_effect(
            &target,
            &ChannelEffect::Pin {
                message_id: "message".to_owned(),
            },
        )?
        .ok_or_else(|| ChannelError::Protocol("telegram pin effect missing".to_owned()))?;
    assert_eq!(pin.path, "pinChatMessage");

    let target = MessageTarget {
        channel: ChannelId::new("discord")?,
        conversation: ConversationId::new("channel")?,
        thread: None,
        reply_to: None,
    };
    let typing = DiscordCodec
        .encode_effect(&target, &ChannelEffect::Typing { active: true })?
        .ok_or_else(|| ChannelError::Protocol("discord effect missing".to_owned()))?;
    assert_eq!(typing.path, "channels/channel/typing");

    let pin = DiscordCodec
        .encode_effect(
            &target,
            &ChannelEffect::Unpin {
                message_id: "message".to_owned(),
            },
        )?
        .ok_or_else(|| ChannelError::Protocol("discord unpin effect missing".to_owned()))?;
    assert_eq!(pin.path, "channels/channel/pins/message");

    let target = MessageTarget {
        channel: ChannelId::new("slack")?,
        conversation: ConversationId::new("C1")?,
        thread: None,
        reply_to: None,
    };
    let edit = SlackCodec
        .encode_effect(
            &target,
            &ChannelEffect::Edit {
                message_id: "1.2".to_owned(),
                body: MessageBody::text("edited")?,
            },
        )?
        .ok_or_else(|| ChannelError::Protocol("slack effect missing".to_owned()))?;
    assert_eq!(edit.path, "chat.update");
    assert!(edit.body.contains("edited"));

    let redact = SlackCodec
        .encode_effect(
            &target,
            &ChannelEffect::Redact {
                message_id: "1.2".to_owned(),
                reason: Some("moderation".to_owned()),
            },
        )?
        .ok_or_else(|| ChannelError::Protocol("slack redact effect missing".to_owned()))?;
    assert_eq!(redact.path, "chat.delete");

    let target = MessageTarget {
        channel: ChannelId::new("matrix")?,
        conversation: ConversationId::new("!room:example.org")?,
        thread: None,
        reply_to: None,
    };
    let delete = MatrixCodec
        .encode_effect(
            &target,
            &ChannelEffect::Delete {
                message_id: "$event".to_owned(),
            },
        )?
        .ok_or_else(|| ChannelError::Protocol("matrix effect missing".to_owned()))?;
    assert_eq!(delete.path, "rooms/!room:example.org/redact/$event");
    Ok(())
}

#[test]
fn choice_command_is_provider_neutral() -> Result<(), ChannelError> {
    let frame = ChannelFrame::new(ChannelFrameBody::Command {
        request_id: "run-2".to_owned(),
        session: "im-chat".to_owned(),
        command_id: "choice-1".to_owned(),
        command: ChannelCommand::RequestChoice {
            question: "Which plan?".to_owned(),
            choices: vec![ChannelChoice {
                id: "safe".to_owned(),
                label: "Safe".to_owned(),
            }],
            multiple: false,
        },
        target: None,
    });
    let encoded = frame
        .encode()
        .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    let decoded = ChannelFrame::decode(&encoded)
        .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    assert_eq!(decoded.frame, frame.frame);
    Ok(())
}

#[test]
fn channel_socket_frame_round_trips_runtime_command_result() -> Result<(), ChannelError> {
    let frame = ChannelFrame::new(ChannelFrameBody::Command {
        request_id: "run-1".to_owned(),
        session: "im-chat".to_owned(),
        command_id: "call-1".to_owned(),
        command: ChannelCommand::RequestApproval {
            tool: "fs.write".to_owned(),
            arguments: serde_json::json!({"path":"notes.txt"}),
        },
        target: Some(MessageTarget {
            channel: ChannelId::from_static("slack"),
            conversation: ConversationId::new("C1")?,
            thread: None,
            reply_to: None,
        }),
    });
    let encoded = frame
        .encode()
        .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    let decoded = ChannelFrame::decode(&encoded)
        .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    assert_eq!(decoded, frame);
    let result = ChannelFrame::new(ChannelFrameBody::CommandResult {
        request_id: "run-1".to_owned(),
        session: "im-chat".to_owned(),
        command_id: "call-1".to_owned(),
        result: ChannelCommandResult::Accepted,
    });
    assert!(result.encode().is_ok());
    Ok(())
}

#[test]
fn channel_control_frame_round_trips_effect_request() -> Result<(), ChannelError> {
    let frame = ChannelFrame::new(ChannelFrameBody::ControlRequest {
        request_id: "tool-1".to_owned(),
        action: ChannelControlAction::Effect {
            target: MessageTarget {
                channel: ChannelId::from_static("discord"),
                conversation: ConversationId::new("room-1")?,
                thread: None,
                reply_to: None,
            },
            effect: ChannelEffect::Typing { active: true },
        },
    });
    let decoded = ChannelFrame::decode(
        &frame
            .encode()
            .map_err(|error| ChannelError::Protocol(error.to_string()))?,
    )
    .map_err(|error| ChannelError::Protocol(error.to_string()))?;
    assert_eq!(decoded, frame);
    Ok(())
}

#[test]
fn channel_socket_frame_rejects_wrong_abi() {
    let frame = ChannelFrame {
        abi: "other".to_owned(),
        frame: ChannelFrameBody::Health {
            health: ChannelHealth::ready(),
        },
    };
    assert!(frame.encode().is_err());
}

#[test]
fn every_catalog_channel_exposes_common_tool_names() {
    for spec in CHANNEL_CATALOG {
        let names = spec.tool_names();
        assert!(names.iter().any(|name| name == "channel.send"));
        assert!(
            names
                .iter()
                .any(|name| name == &format!("{}.invoke", spec.id))
        );
    }
}
