use std::{
    io::Write,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
};

use cortexfs_channels::ChannelFrame;

pub(super) enum Output<'a> {
    Borrowed(&'a mut UnixStream),
    Shared(Arc<Mutex<UnixStream>>),
}

impl Output<'_> {
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching the writer enum keeps exclusive output access"
    )]
    pub(super) fn write(&mut self, frame: &ChannelFrame) -> bool {
        let Ok(bytes) = frame.encode() else {
            return false;
        };
        match self {
            Self::Borrowed(stream) => stream
                .write_all(&bytes)
                .and_then(|()| stream.flush())
                .is_ok(),
            Self::Shared(writer) => writer
                .lock()
                .ok()
                .and_then(|mut stream| stream.write_all(&bytes).and_then(|()| stream.flush()).ok())
                .is_some(),
        }
    }
}
