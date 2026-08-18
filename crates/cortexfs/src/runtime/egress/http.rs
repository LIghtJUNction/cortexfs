mod auth;
mod curl;
mod header;
mod policy;
mod process;
mod request;
mod transfer;

use std::io::{self, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use auth::inject_provider_credential;
use curl::run_curl;
use request::parse_request;

use super::ProviderTarget;

const CLIENT_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Eq, PartialEq)]
struct Request {
    endpoint: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub(super) fn relay(
    mut local: UnixStream,
    target: &ProviderTarget,
    shutdown: &Arc<AtomicBool>,
) -> io::Result<()> {
    local.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    local.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    let mut input = BufReader::new(local.try_clone()?);
    let request = parse_request(&mut input, &mut local, target).inspect_err(|_error| {
        let _ignored = local.write_all(
            b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
    })?;
    let request = inject_provider_credential(request, target);
    run_curl(local, target, &request, shutdown)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
