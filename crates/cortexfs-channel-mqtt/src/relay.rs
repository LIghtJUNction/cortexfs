#![expect(
    clippy::redundant_pub_crate,
    reason = "the relay runner is called by the private binary entry point"
)]

use std::io::Write as _;

use rumqttc::{AsyncClient, Event, Incoming};

use crate::{
    config::Config,
    error::{Error, Result},
    message,
    socket::Session,
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
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
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
    let session = Session::connect(config).await?;
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
                        session.send(message::decode(&publish.topic, &publish.payload)?)?;
                    }
                    Event::Incoming(_) | Event::Outgoing(_) => {}
                }
            }
        }
    }
}

async fn next_frame(session: Session) -> Result<cortexfs_channels::ChannelFrameBody> {
    tokio::task::spawn_blocking(move || session.next())
        .await
        .map_err(|error| Error::Task(error.to_string()))?
}
