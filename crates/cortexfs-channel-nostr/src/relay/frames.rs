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
            let message_id = format!("nostr-{}", target.conversation);
            session.send_receipt(request_id, DeliveryReceipt::new(target, message_id))?;
        }
        ChannelFrameBody::Command {
            request_id,
            session: session_id,
            command_id,
            command: ChannelCommand::Invoke { name, payload },
            target: Some(target),
        } => {
            let result = message::invoke(client, &target, &name, &payload).await;
            let result = ChannelCommandResult::from_value_result(result);
            session.send_command_result(request_id, session_id, command_id, result)?;
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
