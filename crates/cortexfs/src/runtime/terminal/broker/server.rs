use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::handle::serve;
use super::state::BrokerState;
use super::{BrokerProtocolError, BrokerReply, write_frame};

const MAX_HANDSHAKES: usize = 64;

pub fn run(listener: &UnixListener) -> io::Result<()> {
    let state = Arc::new(BrokerState::new());
    let active = Arc::new(AtomicUsize::new(0));
    loop {
        let (stream, _address) = listener.accept()?;
        if !try_acquire(&active) {
            reject_busy(stream);
            continue;
        }
        let state = Arc::clone(&state);
        let active = Arc::clone(&active);
        std::thread::spawn(move || {
            let _guard = WorkerGuard(active);
            serve_one(stream, &state);
        });
    }
}

fn serve_one(mut stream: UnixStream, state: &Arc<BrokerState>) {
    if let Err(error) = serve(&mut stream, state) {
        let reply = error_reply(&error);
        let _result = write_frame(&mut stream, &reply);
    }
}

fn reject_busy(mut stream: UnixStream) {
    let _result = write_frame(
        &mut stream,
        &BrokerReply::Error {
            code: "busy".into(),
            message: "terminal broker handshake limit reached".into(),
        },
    );
}

fn error_reply(error: &BrokerProtocolError) -> BrokerReply {
    let (code, message) = match *error {
        BrokerProtocolError::Rejected(ref code, ref message) => (code.clone(), message.clone()),
        BrokerProtocolError::FrameLimit => (
            "frame_limit".into(),
            "terminal broker frame exceeds limit".into(),
        ),
        _ => ("protocol".into(), "terminal broker request rejected".into()),
    };
    BrokerReply::Error { code, message }
}

fn try_acquire(active: &AtomicUsize) -> bool {
    if active.fetch_add(1, Ordering::AcqRel) < MAX_HANDSHAKES {
        return true;
    }
    active.fetch_sub(1, Ordering::AcqRel);
    false
}

struct WorkerGuard(Arc<AtomicUsize>);

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
