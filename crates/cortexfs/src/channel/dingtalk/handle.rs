use std::{collections::HashMap, net::TcpStream};

use cortexfs_channels::{ChannelCodec, platform::dingtalk::DingTalkCodec};
use reqwest::blocking::Client;
use tungstenite::{Message, WebSocket, connect, stream::MaybeTlsStream};

use super::{DingTalkConfig, DingTalkError, api, parse, transport};
use crate::channel::bridge::AgentChannelBridge;

pub(super) fn run_once(
    config: &DingTalkConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<(), DingTalkError> {
    let gateway = api::register(client, config)?;
    let url = transport::websocket_url(&gateway.endpoint, &gateway.ticket)?;
    let (mut socket, _) = connect(url).map_err(DingTalkError::WebSocket)?;
    let codec = DingTalkCodec;
    let mut webhooks = HashMap::new();
    loop {
        match socket.read()? {
            Message::Text(payload) => handle_frame(
                &mut socket,
                codec,
                &mut webhooks,
                bridge,
                client,
                payload.as_str(),
            )?,
            Message::Ping(payload) => socket.send(Message::Pong(payload))?,
            Message::Close(_) => return Ok(()),
            _ => {}
        }
    }
}

fn handle_frame(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    codec: DingTalkCodec,
    webhooks: &mut HashMap<String, String>,
    bridge: &AgentChannelBridge,
    client: &Client,
    payload: &str,
) -> Result<(), DingTalkError> {
    let root = parse::root(payload)?;
    if parse::frame_type(&root) == Some("SYSTEM") {
        socket.send(Message::text(transport::ack(&root)))?;
        return Ok(());
    }
    let Some(inbound) = codec.decode(payload)? else {
        return Ok(());
    };
    socket.send(Message::text(transport::ack(&root)))?;
    if let Some(webhook) = DingTalkCodec::session_webhook(payload) {
        webhooks.insert(inbound.target.conversation.to_string(), webhook);
        if webhooks.len() > 4096
            && let Some(key) = webhooks.keys().next().cloned()
        {
            webhooks.remove(&key);
        }
    }
    let Ok(outbound) = bridge.handle(inbound) else {
        return Ok(());
    };
    let Some(webhook) = webhooks.get(outbound.target.conversation.as_str()) else {
        return Ok(());
    };
    api::reply(client, webhook, codec.encode(&outbound)?)
}
