use std::{
    os::unix::net::UnixStream,
    sync::{Arc, Mutex, mpsc},
};

use super::DriverConfig;

const SESSION_WORKERS: usize = 4;
const PENDING_SESSIONS: usize = 16;

pub(super) fn spawn(config: &DriverConfig) -> mpsc::SyncSender<UnixStream> {
    let (sender, receiver) = mpsc::sync_channel(PENDING_SESSIONS);
    let config = Arc::new(config.clone());
    let receiver = Arc::new(Mutex::new(receiver));
    for _worker in 0..SESSION_WORKERS {
        let config = Arc::clone(&config);
        let receiver = Arc::clone(&receiver);
        std::thread::spawn(move || serve(&config, &receiver));
    }
    sender
}

fn serve(config: &DriverConfig, receiver: &Mutex<mpsc::Receiver<UnixStream>>) {
    loop {
        let stream = {
            let Ok(receiver) = receiver.lock() else {
                return;
            };
            let Ok(stream) = receiver.recv() else {
                return;
            };
            stream
        };
        let _ignored = super::session::serve(stream, config);
    }
}
