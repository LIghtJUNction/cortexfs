use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::support::process::read_limited_bytes;

use super::process::CurlProcess;

const STDERR_MAX: usize = 16 * 1024;
const IO_PAUSE: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum CurlStop {
    Exited,
    Cancelled,
}

pub(super) fn run(
    process: &mut CurlProcess,
    local: UnixStream,
    config: &str,
    monitor: &UnixStream,
    shutdown: &AtomicBool,
    disconnected: &Arc<AtomicBool>,
) -> io::Result<CurlStop> {
    let (mut stdin, mut stdout, stderr) = process.take_stdio()?;
    stdin.write_all(config.as_bytes())?;
    drop(stdin);
    let output_disconnected = Arc::clone(disconnected);
    process.output = Some(
        thread::Builder::new()
            .name("egress-curl-output".to_owned())
            .spawn(move || {
                let mut local = local;
                io::copy(&mut stdout, &mut local).inspect_err(|_error| {
                    output_disconnected.store(true, Ordering::Release);
                })
            })?,
    );
    process.errors = Some(
        thread::Builder::new()
            .name("egress-curl-errors".to_owned())
            .spawn(move || read_limited_bytes(stderr, STDERR_MAX))?,
    );
    loop {
        if shutdown.load(Ordering::Acquire)
            || disconnected.load(Ordering::Acquire)
            || client_closed(monitor)?
        {
            return Ok(CurlStop::Cancelled);
        }
        if process.child_mut()?.try_wait()?.is_some() {
            return Ok(CurlStop::Exited);
        }
        thread::sleep(IO_PAUSE);
    }
}

fn client_closed(stream: &UnixStream) -> io::Result<bool> {
    #[cfg(test)]
    if super::tests::fail_monitor(stream.as_raw_fd()) {
        return Err(io::Error::from_raw_os_error(nix::libc::EBADF));
    }
    let mut byte = [0_u8; 1];
    match nix::sys::socket::recv(
        stream.as_raw_fd(),
        &mut byte,
        nix::sys::socket::MsgFlags::MSG_PEEK | nix::sys::socket::MsgFlags::MSG_DONTWAIT,
    ) {
        Ok(0) => Ok(true),
        Ok(_) | Err(nix::errno::Errno::EAGAIN) => Ok(false),
        Err(error) => Err(io::Error::from(error)),
    }
}
