#![expect(
    clippy::redundant_pub_crate,
    reason = "the relay runner is called by the private binary entry point"
)]

use std::{collections::BTreeMap, io::Write as _, path::PathBuf, time::Duration};

use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelDriverSession, ChannelFrameBody, ChannelId,
};
use futures_util::StreamExt;
use lapin::{
    Connection, ConnectionProperties,
    options::{BasicConsumeOptions, BasicQosOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
};
use tokio_executor_trait::Tokio as TokioExecutor;
use tokio_reactor_trait::Tokio as TokioReactor;

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
            Err(error) if matches!(error, Error::Config(_)) => return Err(error),
            Err(_error) => {
                let _ignored = writeln!(std::io::stderr(), "cortexfs-channel-amqp: reconnecting");
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(60);
            }
        }
    }
}

async fn run_once(config: &Config) -> Result<()> {
    let properties = ConnectionProperties::default()
        .with_executor(TokioExecutor::current())
        .with_reactor(TokioReactor);
    let connection = Connection::connect(&config.url, properties).await?;
    let channel = connection.create_channel().await?;
    channel
        .basic_qos(config.prefetch, BasicQosOptions::default())
        .await?;
    channel
        .queue_declare(
            &config.queue,
            QueueDeclareOptions {
                durable: config.durable_ack,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    for key in &config.routing_keys {
        channel
            .queue_bind(
                &config.queue,
                &config.exchange,
                key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;
    }
    consume(config, &channel).await
}

async fn consume(config: &Config, channel: &lapin::Channel) -> Result<()> {
    let mut consumer = channel
        .basic_consume(
            &config.queue,
            "cortexfs",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    let session = connect_session(config.socket.clone()).await?;
    let mut pending = BTreeMap::new();
    let mut frame = Box::pin(next_frame(session.clone()));
    loop {
        tokio::select! {
            result = &mut frame => {
                frames::handle(config, channel, &session, &mut pending, result?).await?;
                frame = Box::pin(next_frame(session.clone()));
            }
            result = consumer.next() => {
                let delivery = result.ok_or(Error::Closed)??;
                let inbound = match message::decode(&delivery) {
                    Ok(inbound) => inbound,
                    Err(error) => {
                        frames::reject(config, &delivery).await?;
                        if delivery.redelivered { return Err(error); }
                        continue;
                    }
                };
                if pending.contains_key(&inbound.id) {
                    frames::reject(config, &delivery).await?;
                    continue;
                }
                session.send_inbound(inbound.clone())?;
                pending.insert(inbound.id, delivery);
            }
        }
    }
}

async fn connect_session(path: PathBuf) -> Result<ChannelDriverSession> {
    tokio::task::spawn_blocking(move || -> Result<ChannelDriverSession> {
        Ok(ChannelDriverSession::connect_retry(
            &path,
            &ChannelId::from_static("amqp"),
            ChannelCapabilities {
                tool_control: true,
                ..ChannelCapabilities::text()
            },
            ChannelActions::empty(),
            "amqp",
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
