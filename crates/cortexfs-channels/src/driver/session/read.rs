use std::{
    io::{BufRead, BufReader},
    os::unix::net::UnixStream,
    sync::mpsc::SyncSender,
    thread,
};

use crate::{ChannelFrame, ChannelFrameBody};

use super::super::ChannelDriverError;

pub(super) fn spawn(
    stream: UnixStream,
    sender: SyncSender<Result<ChannelFrameBody, ChannelDriverError>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => match ChannelFrame::decode(&line) {
                    Ok(frame) => {
                        if sender.send(Ok(frame.frame)).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        if sender.send(Err(error.into())).is_err() {
                            return;
                        }
                    }
                },
                Err(error) => {
                    let _ignored = sender.send(Err(ChannelDriverError::Io(error)));
                    return;
                }
            }
        }
        let _ignored = sender.send(Err(ChannelDriverError::Protocol(
            "channel driver closed before next frame".to_owned(),
        )));
    })
}
