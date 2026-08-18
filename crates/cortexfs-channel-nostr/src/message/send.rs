use cortexfs_channels::OutboundMessage;
use nostr_sdk::prelude::{Client, EventBuilder, Kind, NostrSigner, PublicKey, Tag};

use crate::error::{Error, Result, nostr};

use super::Incoming;

pub(crate) async fn reply(
    client: &Client,
    incoming: &Incoming,
    message: OutboundMessage,
) -> Result<()> {
    if message.target.conversation.as_str() != incoming.message.target.conversation.as_str() {
        return Err(Error::Protocol(
            "outbound conversation does not match inbound sender".to_owned(),
        ));
    }
    send(client, incoming.sender, incoming.nip17, message).await
}

pub(crate) async fn proactive(client: &Client, message: OutboundMessage) -> Result<()> {
    let sender = PublicKey::parse(message.target.conversation.as_str())
        .map_err(|error| Error::Protocol(error.to_string()))?;
    let nip17 = message.metadata.get("nostr_protocol").map(String::as_str) == Some("nip17");
    send(client, sender, nip17, message).await
}

async fn send(
    client: &Client,
    sender: PublicKey,
    nip17: bool,
    message: OutboundMessage,
) -> Result<()> {
    if !message.body.attachments.is_empty() || message.body.text.is_empty() {
        return Err(Error::Protocol(
            "Nostr driver supports text replies only".to_owned(),
        ));
    }
    if nip17 {
        client
            .send_private_msg(sender, message.body.text, [])
            .await
            .map_err(nostr)?;
    } else {
        let signer = client.signer().await.map_err(nostr)?;
        let encrypted = signer
            .nip04_encrypt(&sender, &message.body.text)
            .await
            .map_err(nostr)?;
        client
            .send_event_builder(
                EventBuilder::new(Kind::EncryptedDirectMessage, encrypted)
                    .tag(Tag::public_key(sender)),
            )
            .await
            .map_err(nostr)?;
    }
    Ok(())
}
