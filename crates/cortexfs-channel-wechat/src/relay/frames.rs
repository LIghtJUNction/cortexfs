use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelDriverSession, ChannelFrameBody,
    ChannelRuntimeEvent, DeliveryReceipt, OutboundMessage,
};

use crate::{
    api,
    config::Config,
    error::{Error, Result},
    message::Incoming,
};

pub(super) async fn handle(
    client: &reqwest::Client,
    config: &Config,
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
                reply(client, config, &incoming, &message).await?;
            }
        }
        ChannelFrameBody::Outbound {
            request_id,
            message,
        } => {
            proactive(client, config, session, request_id, message).await?;
        }
        ChannelFrameBody::Command {
            request_id,
            session: session_id,
            command_id,
            command,
            target,
        } => {
            let result = match command {
                ChannelCommand::Invoke { name, payload } => {
                    crate::invoke::run(client, config, target.as_ref(), &name, &payload).await
                }
                _ => Err(Error::Protocol("WeChat command is unsupported".to_owned())),
            };
            session.send_command_result(
                request_id,
                session_id,
                command_id,
                ChannelCommandResult::from_value_result(result),
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

async fn reply(
    client: &reqwest::Client,
    config: &Config,
    incoming: &Incoming,
    message: &OutboundMessage,
) -> Result<()> {
    validate(message)?;
    let user = message
        .metadata
        .get("wechat_user_id")
        .map_or(incoming.message.sender.id.as_str(), String::as_str);
    let context = message
        .metadata
        .get("wechat_context_token")
        .map_or(incoming.context_token.as_str(), String::as_str);
    api::send_message(client, config, user, context, &message.body.text).await
}

async fn proactive(
    client: &reqwest::Client,
    config: &Config,
    session: &ChannelDriverSession,
    request_id: String,
    message: OutboundMessage,
) -> Result<()> {
    validate(&message)?;
    let user = message
        .metadata
        .get("wechat_user_id")
        .map_or(message.target.conversation.as_str(), String::as_str);
    let context = message
        .metadata
        .get("wechat_context_token")
        .map_or("", String::as_str);
    api::send_message(client, config, user, context, &message.body.text).await?;
    let message_id = format!("wechat-{}", message.target.conversation);
    Ok(session.send_receipt(request_id, DeliveryReceipt::new(message.target, message_id))?)
}

fn validate(message: &OutboundMessage) -> Result<()> {
    if message.body.text.is_empty() || !message.body.attachments.is_empty() {
        return Err(Error::Protocol(
            "WeChat driver supports text replies only".to_owned(),
        ));
    }
    Ok(())
}
