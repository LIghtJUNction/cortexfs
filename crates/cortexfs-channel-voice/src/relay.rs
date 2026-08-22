#![expect(
    clippy::redundant_pub_crate,
    reason = "the relay runner is called by the private binary entry point"
)]

use std::{collections::BTreeMap, io::Write as _, time::Duration};

use cortexfs_channels::{ChannelActions, ChannelDriverSession, ChannelFrameBody, ChannelId};
use reqwest::Client;
use tokio::sync::mpsc;

use crate::{
    config::{ChannelKind, Config},
    error::{Error, Result},
    http,
};

mod frames;
mod webhook;

pub(crate) async fn run(config: Config) -> Result<()> {
    let mut delay = Duration::from_secs(1);
    loop {
        match run_once(&config).await {
            Ok(()) => return Ok(()),
            Err(error) if matches!(error, Error::Config(_)) => return Err(error),
            Err(_error) => {
                let _ignored = writeln!(std::io::stderr(), "cortexfs-channel-voice: reconnecting");
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_mins(1));
            }
        }
    }
}

async fn run_once(config: &Config) -> Result<()> {
    if config.channel == ChannelKind::VoiceWake {
        return run_wake_once(config).await;
    }
    let session = connect_session(config).await?;
    let client = Client::new();
    let (sender, mut receiver) = mpsc::channel(64);
    let mut webhook = tokio::spawn(http::serve(
        config.webhook_bind,
        config.webhook_token.clone(),
        sender,
    ));
    let mut calls = BTreeMap::new();
    let mut frame = Box::pin(next_frame(session.clone()));
    loop {
        tokio::select! {
            result = &mut frame => {
                frames::handle(config, &client, &session, &mut calls, result?).await?;
                frame = Box::pin(next_frame(session.clone()));
            }
            event = receiver.recv() => {
                let event = event.ok_or_else(|| Error::Protocol("webhook queue closed".to_owned()))?;
                webhook::handle(config, &session, &mut calls, &event)?;
            }
            result = &mut webhook => {
                result.map_err(|error| Error::Task(error.to_string()))??;
                return Err(Error::Protocol("webhook server stopped".to_owned()));
            }
        }
    }
}

async fn run_wake_once(config: &Config) -> Result<()> {
    let session = connect_session(config).await?;
    let client = Client::new();
    let mut calls = BTreeMap::new();
    let mut frame = Box::pin(next_frame(session.clone()));
    loop {
        frames::handle(config, &client, &session, &mut calls, frame.await?).await?;
        frame = Box::pin(next_frame(session.clone()));
    }
}

async fn connect_session(config: &Config) -> Result<ChannelDriverSession> {
    let path = config.socket.clone();
    let channel = ChannelId::new(config.channel.id())?;
    let capabilities = Config::capabilities(config.channel);
    tokio::task::spawn_blocking(move || -> Result<ChannelDriverSession> {
        Ok(ChannelDriverSession::connect_retry(
            &path,
            &channel,
            capabilities,
            ChannelActions::empty(),
            "voice",
            Duration::from_secs(10),
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
