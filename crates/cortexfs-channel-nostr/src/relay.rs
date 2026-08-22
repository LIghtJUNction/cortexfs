#![expect(
    clippy::redundant_pub_crate,
    reason = "the relay runner is called by the private binary entry point"
)]

use std::{future::Future, path::PathBuf, pin::Pin, sync::Arc, time::Duration};

use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelDriverSession, ChannelFrameBody, ChannelId,
};
use nostr_sdk::prelude::{Filter, Kind, Timestamp};
use nostr_sdk::{Client, ClientBuilder, RelayPoolNotification};
use tokio::sync::mpsc;

use crate::{
    config::Config,
    error::{Error, Result, nostr},
    message::{self, Incoming},
};

mod frames;

const MAX_PENDING: usize = 64;

pub(crate) async fn run(config: Config) -> Result<()> {
    let config = Arc::new(config);
    let client = build_client(&config).await?;
    let session = connect_session(config.socket.clone(), config.reply_timeout).await?;
    let (sender, receiver) = mpsc::channel(MAX_PENDING);
    let notifications = Box::pin(notifications(client.clone(), Arc::clone(&config), sender));
    event_loop(client, session, receiver, notifications).await
}

async fn build_client(config: &Config) -> Result<Client> {
    let client = ClientBuilder::new().signer(config.keys.clone()).build();
    for relay in &config.relays {
        client.add_relay(relay).await.map_err(nostr)?;
    }
    client.connect().await;
    client
        .subscribe(
            Filter::new()
                .pubkey(config.keys.public_key())
                .kinds([Kind::EncryptedDirectMessage, Kind::GiftWrap])
                .since(Timestamp::now())
                .limit(100),
            None,
        )
        .await
        .map_err(nostr)?;
    Ok(client)
}

async fn notifications(
    client: Client,
    config: Arc<Config>,
    sender: mpsc::Sender<Incoming>,
) -> Result<()> {
    let worker_client = client.clone();
    client
        .handle_notifications(move |notification| {
            let client = worker_client.clone();
            let config = Arc::clone(&config);
            let sender = sender.clone();
            async move {
                match notification {
                    RelayPoolNotification::Event { event, .. } => {
                        if let Some(incoming) = message::decode(&client, &event, &config).await? {
                            sender
                                .send(incoming)
                                .await
                                .map_err(|_error| nostr("event queue closed"))?;
                        }
                        Ok(false)
                    }
                    RelayPoolNotification::Shutdown => Ok(true),
                    RelayPoolNotification::Message { .. } => Ok(false),
                }
            }
        })
        .await
        .map_err(nostr)
}

async fn event_loop(
    client: Client,
    session: ChannelDriverSession,
    mut receiver: mpsc::Receiver<Incoming>,
    mut notifications: Pin<Box<impl Future<Output = Result<()>>>>,
) -> Result<()> {
    let mut pending = std::collections::BTreeMap::new();
    let mut frame = Box::pin(next_frame(session.clone()));
    loop {
        tokio::select! {
            result = &mut frame => {
                frames::handle(&client, &session, &mut pending, result?).await?;
                frame = Box::pin(next_frame(session.clone()));
            }
            Some(incoming) = receiver.recv() => {
                if pending.len() >= MAX_PENDING {
                    return Err(Error::Protocol("too many pending Nostr replies".to_owned()));
                }
                session.send_inbound(incoming.message.clone())?;
                pending.insert(incoming.message.id.clone(), incoming);
            }
            result = &mut notifications => return result,
        }
    }
}

async fn connect_session(path: PathBuf, timeout: Duration) -> Result<ChannelDriverSession> {
    tokio::task::spawn_blocking(move || -> Result<ChannelDriverSession> {
        Ok(ChannelDriverSession::connect_retry(
            &path,
            &ChannelId::from_static("nostr"),
            ChannelCapabilities {
                tool_control: true,
                websocket: true,
                ..ChannelCapabilities::text()
            },
            ChannelActions::empty(),
            "nostr",
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
