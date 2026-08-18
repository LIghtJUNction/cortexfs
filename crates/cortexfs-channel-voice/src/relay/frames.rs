use cortexfs_channels::{ChannelFrameBody, ChannelRuntimeEvent, DeliveryReceipt};
use reqwest::Client;

use crate::{
    config::Config,
    error::{Error, Result},
    provider::{self, Calls},
    socket::Session,
};

pub(super) async fn handle(
    config: &Config,
    client: &Client,
    session: &Session,
    calls: &mut Calls,
    frame: ChannelFrameBody,
) -> Result<()> {
    let (request_id, message) = match frame {
        ChannelFrameBody::Deliver {
            request_id,
            message,
        }
        | ChannelFrameBody::Outbound {
            request_id,
            message,
        } => (request_id, message),
        ChannelFrameBody::Event {
            event: ChannelRuntimeEvent::Disconnected,
        } => {
            return Err(Error::Protocol("channel driver disconnected".to_owned()));
        }
        ChannelFrameBody::Error {
            request_id: Some(request_id),
            ..
        } => {
            calls.retain(|_, call| call.id != request_id);
            return Ok(());
        }
        _ => return Ok(()),
    };
    let target = message.target.clone();
    let id = provider::send(config, client, calls, &message).await?;
    session.receipt(
        request_id,
        DeliveryReceipt {
            channel: target.channel.clone(),
            message_id: format!("voice-{id}"),
            target,
            timestamp_ms: None,
        },
    )
}
