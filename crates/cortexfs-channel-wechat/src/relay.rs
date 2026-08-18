#![expect(
    clippy::redundant_pub_crate,
    reason = "the relay runner is called by the private binary entry point"
)]

use std::{collections::BTreeMap, io::Write as _, time::Duration};

use cortexfs_channels::ChannelFrameBody;
use serde_json::Value;

use crate::{
    api,
    config::Config,
    error::{Error, Result},
    message::{self, Incoming},
    socket,
};

mod frames;

const MAX_PENDING: usize = 64;

pub(crate) async fn run(config: Config) -> Result<()> {
    let mut delay = Duration::from_secs(1);
    loop {
        match run_once(&config).await {
            Ok(()) => return Ok(()),
            Err(error) if matches!(error, Error::Config(_)) => return Err(error),
            Err(error) => {
                let _ignored = writeln!(std::io::stderr(), "cortexfs-channel-wechat: reconnecting");
                let _ = error;
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_mins(1));
            }
        }
    }
}

async fn run_once(config: &Config) -> Result<()> {
    let client = api::client(config)?;
    let session = socket::Session::connect(config.socket.clone(), config.reply_timeout).await?;
    let mut cursor = String::new();
    let mut pending = BTreeMap::new();
    let mut frame = Box::pin(next_frame(session.clone()));
    let mut poll = Box::pin(poll_updates(&client, config, cursor.clone()));
    loop {
        tokio::select! {
            result = &mut frame => {
                frames::handle(&client, config, &session, &mut pending, result?).await?;
                frame = Box::pin(next_frame(session.clone()));
            }
            result = &mut poll => {
                let batch = result?;
                cursor = batch.cursor;
                for value in batch.messages {
                    enqueue(config, &session, &mut pending, &value)?;
                }
                poll = Box::pin(poll_updates(&client, config, cursor.clone()));
            }
        }
    }
}

async fn poll_updates(
    client: &reqwest::Client,
    config: &Config,
    cursor: String,
) -> Result<api::UpdateBatch> {
    api::get_updates(client, config, &cursor).await
}

async fn next_frame(session: socket::Session) -> Result<ChannelFrameBody> {
    tokio::task::spawn_blocking(move || session.next())
        .await
        .map_err(|error| Error::Task(error.to_string()))?
}

fn enqueue(
    config: &Config,
    session: &socket::Session,
    pending: &mut BTreeMap<String, Incoming>,
    value: &Value,
) -> Result<()> {
    let Some(incoming) = message::decode(value, config)? else {
        return Ok(());
    };
    if pending.len() >= MAX_PENDING {
        return Err(Error::Protocol(
            "too many pending WeChat replies".to_owned(),
        ));
    }
    let id = incoming.message.id.clone();
    session.send_inbound(incoming.message.clone())?;
    pending.insert(id, incoming);
    Ok(())
}
