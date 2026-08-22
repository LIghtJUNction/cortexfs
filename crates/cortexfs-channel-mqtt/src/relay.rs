#![expect(
    clippy::redundant_pub_crate,
    reason = "the relay runner is called by the private binary entry point"
)]

use std::{io::Write as _, time::Duration};

use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelDriverSession, ChannelFrameBody, ChannelId,
};
use rumqttc::{AsyncClient, Event, Incoming};

use crate::{
    config::Config,
    error::{Error, Result},
    message,
};

mod frames;

pub(crate) async fn run(config: Config) -> Result<()> {
    let mut delay = 1;
    loop {
        match run_once(&config).await {
            Ok(()) => return Ok(()),
            Err(Error::Config(error)) => return Err(Error::Config(error)),
            Err(_error) => {
                let _ignored = writeln!(std::io::stderr(), "cortexfs-channel-mqtt: reconnecting");
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(60);
            }
        }
    }
}

async fn run_once(config: &Config) -> Result<()> {
    let options = config.mqtt_options()?;
    let (client, mut eventloop) = AsyncClient::new(options, 64);
    for topic in &config.topics {
        client.subscribe(topic, config.qos).await?;
    }
    let session = connect_session(config).await?;
    let mut frame = Box::pin(next_frame(session.clone()));
    loop {
        tokio::select! {
            result = &mut frame => {
                frames::handle(config, &client, &session, result?).await?;
                frame = Box::pin(next_frame(session.clone()));
            }
            result = eventloop.poll() => {
                match result? {
                    Event::Incoming(Incoming::Publish(publish)) => {
                        session.send_inbound(message::decode(&publish.topic, &publish.payload)?)?;
                    }
                    Event::Incoming(_) | Event::Outgoing(_) => {}
                }
            }
        }
    }
}

async fn connect_session(config: &Config) -> Result<ChannelDriverSession> {
    let path = config.socket.clone();
    tokio::task::spawn_blocking(move || -> Result<ChannelDriverSession> {
        Ok(ChannelDriverSession::connect_retry(
            &path,
            &ChannelId::from_static("mqtt"),
            ChannelCapabilities {
                tool_control: true,
                ..ChannelCapabilities::text()
            },
            ChannelActions::empty(),
            "mqtt",
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
