use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelDriverSession, ChannelFrameBody,
    ChannelRuntimeEvent, DeliveryReceipt,
};
use nostr_sdk::Client;

use crate::{
    error::{Error, Result},
    message::{self, Incoming},
};

pub(super) async fn handle(
    client: &Client,
    session: &ChannelDriverSession,
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
            session.send_receipt(
                request_id,
                DeliveryReceipt {
                    channel: target.channel.clone(),
                    message_id: format!("nostr-{}", target.conversation),
                    target,
                    timestamp_ms: None,
                },
            )?;
        }
        ChannelFrameBody::Command {
            request_id,
            session: session_id,
            command_id,
            command: ChannelCommand::Invoke { name, payload },
            target: Some(target),
        } => {
            let result = message::invoke(client, &target, &name, &payload).await;
            let result = result.map_or_else(
                |error| ChannelCommandResult::Rejected {
                    reason: error.to_string(),
                },
                |payload| ChannelCommandResult::Value { payload },
            );
            session.send_frame(ChannelFrameBody::CommandResult {
                request_id,
                session: session_id,
                command_id,
                result,
            })?;
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
