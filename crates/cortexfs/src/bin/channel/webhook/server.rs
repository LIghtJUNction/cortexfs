use std::{
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use cortexfs::channel::{bridge::AgentChannelBridge, http};
use cortexfs_channels::ChannelCodec;

use super::{WebhookConfig, WebhookError, codec, handle};

const WORKERS: usize = 4;
const QUEUE: usize = 16;

pub(super) fn run(config: &WebhookConfig, bridge: &AgentChannelBridge) -> Result<(), WebhookError> {
    let listener = TcpListener::bind(config.bind).map_err(WebhookError::Io)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_mins(1))
        .build()
        .map_err(WebhookError::Http)?;
    let codec: Arc<dyn ChannelCodec> = Arc::from(codec(config.platform));
    let _control = super::control::start(config, bridge, &client, Arc::clone(&codec))?;
    let (sender, receiver) = mpsc::sync_channel(QUEUE);
    let receiver = Arc::new(Mutex::new(receiver));
    let config = Arc::new(config.clone());
    let bridge = Arc::new(bridge.clone());
    for index in 0..WORKERS {
        let receiver = Arc::clone(&receiver);
        let config = Arc::clone(&config);
        let client = client.clone();
        let codec = Arc::clone(&codec);
        let bridge = Arc::clone(&bridge);
        thread::Builder::new()
            .name(format!("cortexfs-webhook-{index}"))
            .spawn(move || worker(receiver, config, client, codec, bridge))
            .map_err(WebhookError::Io)?;
    }
    for stream in listener.incoming() {
        sender
            .send(stream.map_err(WebhookError::Io)?)
            .map_err(|_error| WebhookError::QueueClosed)?;
    }
    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the worker owns reference-counted state for its thread lifetime"
)]
fn worker(
    receiver: Arc<Mutex<mpsc::Receiver<TcpStream>>>,
    config: Arc<WebhookConfig>,
    client: reqwest::blocking::Client,
    codec: Arc<dyn ChannelCodec>,
    bridge: Arc<AgentChannelBridge>,
) {
    loop {
        let stream = receiver.lock().ok().and_then(|queue| queue.recv().ok());
        let Some(mut stream) = stream else {
            return;
        };
        let _ignored = http::serve_stream_once(&mut stream, |request| {
            handle(&config, &client, codec.as_ref(), &bridge, &request)
        });
    }
}
