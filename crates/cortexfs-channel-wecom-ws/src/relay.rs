#![expect(
    clippy::redundant_pub_crate,
    reason = "the relay runner is called by the private binary entry point"
)]

use std::{collections::BTreeMap, io::Write as _, path::PathBuf, sync::Arc, time::Duration};

use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelDriverSession, ChannelFrameBody, ChannelId,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};

use crate::{
    config::Config,
    error::{Error, Result},
    message::InboundEvent,
};

mod frames;
mod input;
mod subscribe;
mod token;

const URL: &str = "wss://openws.work.weixin.qq.com";

pub(crate) async fn run(config: Config) -> Result<()> {
    let config = Arc::new(config);
    let mut delay = 1;
    loop {
        match connect_async(URL).await {
            Ok((stream, _)) => match run_once(stream, Arc::clone(&config)).await {
                Ok(()) => return Ok(()),
                Err(error) if matches!(error, Error::Config(_)) => return Err(error),
                Err(_error) => {}
            },
            Err(_error) => {}
        }
        let _ignored = writeln!(std::io::stderr(), "cortexfs-channel-wecom-ws: reconnecting");
        tokio::time::sleep(Duration::from_secs(delay)).await;
        delay = (delay * 2).min(60);
    }
}

async fn run_once(
    stream: WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    config: Arc<Config>,
) -> Result<()> {
    let session = connect_session(config.socket.clone(), config.reply_timeout).await?;
    let (mut writer, mut reader) = stream.split();
    subscribe::run(&mut writer, &mut reader, &config).await?;
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(32);
    let mut pending = BTreeMap::<String, InboundEvent>::new();
    let mut frame = Box::pin(next_frame(session.clone()));
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                let value = serde_json::json!({"cmd":"ping","headers":{"req_id": token::token()}});
                writer.send(Message::Text(value.to_string().into())).await?;
            }
            Some(value) = out_rx.recv() => writer.send(value).await?,
            result = &mut frame => {
                frames::handle(&session, &out_tx, &mut pending, result?).await?;
                frame = Box::pin(next_frame(session.clone()));
            }
            Some(result) = reader.next() => {
                if !input::receive(result, &config, &session, &out_tx, &mut pending).await? { break; }
            }
            else => break,
        }
    }
    Err(Error::Protocol("WeCom WebSocket closed".to_owned()))
}

async fn connect_session(path: PathBuf, timeout: Duration) -> Result<ChannelDriverSession> {
    tokio::task::spawn_blocking(move || -> Result<ChannelDriverSession> {
        Ok(ChannelDriverSession::connect_retry(
            &path,
            &ChannelId::from_static("wecom-ws"),
            ChannelCapabilities {
                group: true,
                streaming: true,
                websocket: true,
                tool_control: true,
                ..ChannelCapabilities::text()
            },
            ChannelActions::empty(),
            "wecom-ws",
            timeout,
        )?)
    })
    .await
    .map_err(|error| Error::Task(error.to_string()))?
}

async fn next_frame(session: ChannelDriverSession) -> Result<ChannelFrameBody> {
    Ok(tokio::task::spawn_blocking(move || session.recv())
        .await
        .map_err(|error| Error::Task(error.to_string()))??)
}
