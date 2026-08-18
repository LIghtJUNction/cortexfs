use std::{
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, mpsc},
    thread,
};

use cortexfs::channel::http;

use super::{WebConfig, WebError, handle, socket};

const WORKERS: usize = 4;
const QUEUE: usize = 16;

pub(super) fn run(config: &WebConfig) -> Result<(), WebError> {
    let listener = TcpListener::bind(config.bind).map_err(WebError::Io)?;
    let (sender, receiver) = mpsc::sync_channel(QUEUE);
    let receiver = Arc::new(Mutex::new(receiver));
    let config = Arc::new(config.clone());
    for index in 0..WORKERS {
        let receiver = Arc::clone(&receiver);
        let config = Arc::clone(&config);
        thread::Builder::new()
            .name(format!("cortexfs-web-{index}"))
            .spawn(move || worker(receiver, config))
            .map_err(WebError::Io)?;
    }
    for stream in listener.incoming() {
        sender
            .send(stream.map_err(WebError::Io)?)
            .map_err(|_error| WebError::Closed)?;
    }
    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the worker owns reference-counted state for its thread lifetime"
)]
fn worker(receiver: Arc<Mutex<mpsc::Receiver<TcpStream>>>, config: Arc<WebConfig>) {
    loop {
        let stream = receiver.lock().ok().and_then(|queue| queue.recv().ok());
        let Some(stream) = stream else {
            return;
        };
        if socket::is_upgrade(&stream) {
            let _ignored = socket::serve(stream, &config);
            continue;
        }
        let mut stream = stream;
        let _ignored = http::serve_stream_once(&mut stream, |request| handle(&config, &request));
    }
}
