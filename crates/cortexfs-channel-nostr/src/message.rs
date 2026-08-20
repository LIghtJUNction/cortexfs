#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "message values cross private driver modules"
)]

use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget, OutboundMessage,
    Participant,
};
use nostr_sdk::prelude::{Client, Event, EventBuilder, EventId, Kind, NostrSigner, PublicKey, Tag};
use nostr_sdk::serde_json::{Value, json};

use crate::{
    config::Config,
    error::{Error, Result, nostr},
};

mod send;
pub(crate) use send::{proactive, reply};

pub(crate) struct Incoming {
    pub(crate) sender: PublicKey,
    pub(crate) message: InboundMessage,
    pub(crate) nip17: bool,
}

pub(crate) async fn decode(
    client: &Client,
    event: &Event,
    config: &Config,
) -> Result<Option<Incoming>> {
    let (sender, text, nip17) = match event.kind {
        Kind::EncryptedDirectMessage => {
            let signer = client.signer().await.map_err(nostr)?;
            let text = signer
                .nip04_decrypt(&event.pubkey, &event.content)
                .await
                .map_err(nostr)?;
            (event.pubkey, text, false)
        }
        Kind::GiftWrap => {
            let gift = client.unwrap_gift_wrap(event).await.map_err(nostr)?;
            (gift.sender, gift.rumor.content, true)
        }
        _ => return Ok(None),
    };
    if !config.accepts(&sender) || text.is_empty() {
        return Ok(None);
    }
    let conversation = ConversationId::new(sender.to_string())
        .map_err(|error| Error::Protocol(error.to_string()))?;
    let target = MessageTarget {
        channel: ChannelId::from_static("nostr"),
        conversation,
        thread: None,
        reply_to: None,
    };
    let body = MessageBody::text(text).map_err(|error| Error::Protocol(error.to_string()))?;
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "nostr_protocol".to_owned(),
        if nip17 { "nip17" } else { "nip04" }.to_owned(),
    );
    Ok(Some(Incoming {
        sender,
        message: InboundMessage {
            id: event.id.to_string(),
            target,
            sender: Participant {
                id: sender.to_string(),
                ..Participant::default()
            },
            body,
            timestamp_ms: Some(event.created_at.as_secs().saturating_mul(1000)),
            metadata,
        },
        nip17,
    }))
}

pub(crate) async fn invoke(
    client: &Client,
    target: &MessageTarget,
    name: &str,
    payload: &Value,
) -> Result<Value> {
    match name {
        "nostr.publish" => {
            let text = string(payload, "text")?;
            client
                .send_event_builder(EventBuilder::text_note(text))
                .await
                .map_err(nostr)?;
            Ok(json!({"accepted":true}))
        }
        "nostr.send_dm" => {
            let text = string(payload, "text")?;
            let mut metadata = BTreeMap::new();
            metadata.insert(
                "nostr_protocol".to_owned(),
                payload
                    .get("protocol")
                    .and_then(Value::as_str)
                    .unwrap_or("nip17")
                    .to_owned(),
            );
            let message = OutboundMessage {
                target: target.clone(),
                body: MessageBody::text(text.to_owned())
                    .map_err(|error| Error::Protocol(error.to_string()))?,
                metadata,
            };
            proactive(client, message).await?;
            Ok(json!({"accepted":true}))
        }
        "nostr.query_relays" => {
            let relays = client
                .relays()
                .await
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            Ok(json!({"relays": relays}))
        }
        "nostr.reaction" => {
            let event = EventId::parse(string(payload, "event_id")?)
                .map_err(|error| Error::Protocol(error.to_string()))?;
            let reaction = string(payload, "reaction").unwrap_or("+");
            client
                .send_event_builder(
                    EventBuilder::new(Kind::Reaction, reaction).tag(Tag::event(event)),
                )
                .await
                .map_err(nostr)?;
            Ok(json!({"accepted":true}))
        }
        _ => Err(Error::Protocol("unsupported operation".to_owned())),
    }
}

fn string<'a>(value: &'a Value, name: &'static str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| Error::Protocol(format!("{name} is missing")))
}
