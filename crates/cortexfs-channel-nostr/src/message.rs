#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "message values cross private driver modules"
)]

use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget, Participant,
};
use nostr_sdk::prelude::{Client, Event, Kind, NostrSigner, PublicKey};

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
