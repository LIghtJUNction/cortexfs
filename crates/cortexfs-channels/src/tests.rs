use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_core::Stream;

use crate::platform::{
    ChannelCodec, discord::DiscordCodec, feishu::FeishuCodec, slack::SlackCodec,
    telegram::TelegramCodec,
};
use crate::*;

struct EmptyStream;

impl Stream for EmptyStream {
    type Item = Result<InboundMessage, ChannelError>;

    fn poll_next(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

#[derive(Debug)]
struct TestAdapter {
    id: ChannelId,
}

impl ChannelAdapter for TestAdapter {
    fn id(&self) -> ChannelId {
        self.id.clone()
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities::text()
    }

    fn listen(&self) -> Result<ChannelStream, ChannelError> {
        Ok(Box::pin(EmptyStream))
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
fn channel_ids_are_canonical() {
    assert!(ChannelId::new("Telegram").is_err());
    assert_eq!(
        ChannelId::new("telegram").map(|id| id.to_string()),
        Ok("telegram".to_owned())
    );
}

#[test]
fn registry_rejects_duplicate_adapter() -> Result<(), ChannelError> {
    let id = ChannelId::new("test")?;
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(TestAdapter { id: id.clone() }))?;
    assert_eq!(
        registry.register(Arc::new(TestAdapter { id: id.clone() })),
        Err(ChannelError::DuplicateChannel("test".to_owned()))
    );
    assert_eq!(registry.ids(), vec![id]);
    Ok(())
}

#[test]
fn empty_message_body_is_rejected() {
    assert!(MessageBody::default().validate().is_err());
}

#[test]
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
    Ok(())
}
