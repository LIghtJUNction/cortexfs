use std::collections::BTreeMap;

use cortexfs_channels::{ChannelFrameBody, ChannelRuntimeEvent, DeliveryReceipt};
use nostr_sdk::Client;

use crate::{
    error::{Error, Result},
    message::{self, Incoming},
    socket,
};

pub(super) async fn handle(
    client: &Client,
    session: &socket::Session,
    pending: &mut BTreeMap<String, Incoming>,
    frame: ChannelFrameBody,
) -> Result<()> {
    match frame {
        ChannelFrameBody::Deliver {
            request_id,
            message,
        } => {
            if let Some(incoming) = pending.remove(&request_id) {
                message::reply(client, &incoming, message).await?;
            }
        }
        ChannelFrameBody::Outbound {
            request_id,
            message,
        } => {
            let target = message.target.clone();
            message::proactive(client, message).await?;
            session.receipt(
                request_id,
                DeliveryReceipt {
                    channel: target.channel.clone(),
                    message_id: format!("nostr-{}", target.conversation),
                    target,
                    timestamp_ms: None,
                },
            )?;
        }
        ChannelFrameBody::Event {
            event: ChannelRuntimeEvent::Disconnected,
        } => return Err(Error::Protocol("channel driver disconnected".to_owned())),
        ChannelFrameBody::Error {
            request_id: Some(request_id),
            ..
        } => {
            let _ignored = pending.remove(&request_id);
        }
        _ => {}
    }
    Ok(())
}
