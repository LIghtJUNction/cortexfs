#![expect(
    clippy::redundant_pub_crate,
    reason = "the relay runner is called by the private binary entry point"
)]

use std::sync::Arc;

use futures_util::StreamExt;
use reqwest::Client;
use tokio_tungstenite::connect_async;

use crate::{
    api,
    config::Config,
    error::{Error, Result},
    socket::Session,
};

mod frames;
mod input;
mod invoke;

pub(crate) async fn run(config: Config) -> Result<()> {
    let config = Arc::new(config);
    let client = Client::builder().build().map_err(Error::Http)?;
    let mut delay = 1;
    loop {
        match run_once(Arc::clone(&config), client.clone()).await {
            Ok(()) => return Ok(()),
            Err(Error::Config(message)) => return Err(Error::Config(message)),
            Err(_error) => {
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                delay = (delay * 2).min(config.reconnect_seconds.max(1));
            }
        }
    }
}

async fn run_once(config: Arc<Config>, client: Client) -> Result<()> {
    let session = Session::connect(&config).await?;
    let url = api::open_url(&client, &config).await?;
    let (stream, _) = connect_async(url).await?;
    let (mut writer, mut reader) = stream.split();
    let mut frame = Box::pin(next_frame(session.clone()));
    loop {
        tokio::select! {
            result = &mut frame => {
                frames::handle(&client, &config, &session, result?).await?;
                frame = Box::pin(next_frame(session.clone()));
            }
            Some(result) = reader.next() => {
                if !input::handle(&session, &mut writer, result?).await? { break; }
            }
            else => break,
        }
    }
    Err(Error::Api("Slack Socket Mode connection closed".to_owned()))
}

async fn next_frame(session: Session) -> Result<cortexfs_channels::ChannelFrameBody> {
    tokio::task::spawn_blocking(move || session.next())
        .await
        .map_err(|error| Error::Task(error.to_string()))?
}
