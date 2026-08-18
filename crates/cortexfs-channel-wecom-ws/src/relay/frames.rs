use std::collections::BTreeMap;

use cortexfs_channels::{ChannelFrameBody, ChannelRuntimeEvent, DeliveryReceipt, OutboundMessage};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    error::{Error, Result},
    message::InboundEvent,
    output, socket,
};

pub(super) async fn handle(
    session: &socket::Session,
    output_tx: &mpsc::Sender<Message>,
    pending: &mut BTreeMap<String, InboundEvent>,
    frame: ChannelFrameBody,
) -> Result<()> {
    match frame {
        ChannelFrameBody::Deliver {
            request_id,
            message,
        } => {
            if let Some(event) = pending.remove(&request_id) {
                reply(output_tx, &event.request_id, &message).await?;
            }
        }
        ChannelFrameBody::Outbound {
            request_id,
            message,
        } => {
            proactive(session, output_tx, request_id, message).await?;
        }
        ChannelFrameBody::Event {
            event: ChannelRuntimeEvent::Disconnected,
        } => {
            return Err(Error::Protocol("channel driver disconnected".to_owned()));
        }
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

async fn reply(
    output_tx: &mpsc::Sender<Message>,
    request_id: &str,
    message: &OutboundMessage,
) -> Result<()> {
    for frame in output::reply_frames(request_id, &message.body.text) {
        send(output_tx, frame).await?;
    }
    Ok(())
}

async fn proactive(
    session: &socket::Session,
    output_tx: &mpsc::Sender<Message>,
    request_id: String,
    message: OutboundMessage,
) -> Result<()> {
    let target = message.target.clone();
    let runtime_request_id = request_id;
    let platform_request_id = message
        .metadata
        .get("wecom_req_id")
        .cloned()
        .unwrap_or_else(|| runtime_request_id.clone());
    reply(output_tx, &platform_request_id, &message).await?;
    session.receipt(
        runtime_request_id,
        DeliveryReceipt {
            channel: target.channel.clone(),
            message_id: format!("wecom-{}", target.conversation),
            target,
            timestamp_ms: None,
        },
    )
}

async fn send(output_tx: &mpsc::Sender<Message>, text: String) -> Result<()> {
    output_tx
        .send(Message::Text(text.into()))
        .await
        .map_err(|_error| Error::Protocol("WeCom output queue closed".to_owned()))
}
