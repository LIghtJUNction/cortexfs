use std::{
    io::{BufRead, BufReader},
    os::unix::net::UnixStream,
    sync::mpsc::SyncSender,
};

use cortexfs_channels::ChannelFrame;

use super::session::SessionEvent;

pub(super) fn spawn(
    stream: UnixStream,
    events: SyncSender<SessionEvent>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => match ChannelFrame::decode(&line) {
                    Ok(frame) => {
                        if events.send(SessionEvent::Frame(frame)).is_err() {
                            break;
                        }
                    }
                    Err(_error) => {
                        if events.send(SessionEvent::Invalid).is_err() {
                            break;
                        }
                    }
                },
                Err(_error) => break,
            }
        }
        let _ignored = events.send(SessionEvent::Closed);
    })
}
