use std::env;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::*;

const MAX_AUTH_RESPONSE_BYTES: usize = 16 * 1024;

mod browser;
mod device;

pub(crate) use browser::oauth_browser_login;
pub(crate) use device::oauth_device_login;

pub(super) fn api_key_login(provider: &str, profile: &str, key: &str) -> Result<(), CliError> {
    let request_id = format!("auth-{}", hex_bytes(&read_system_entropy(16)?));
    let frame = cortexfs::AuthWireFrame::new(cortexfs::AuthWireRequest::ApiKey {
        request_id: request_id.clone(),
        provider: provider.to_owned(),
        profile: profile.to_owned(),
        key: key.to_owned(),
    });
    run_auth_request(&frame, &request_id, |_| Ok(()))
}

fn run_auth_request(
    frame: &cortexfs::AuthWireFrame<cortexfs::AuthWireRequest>,
    request_id: &str,
    progress: impl FnMut(Option<&str>) -> Result<(), CliError>,
) -> Result<(), CliError> {
    let (mut request, runner_input) = UnixStream::pair()
        .map_err(|_error| CliError::unavailable("cannot create auth request socket"))?;
    let (runner_output, response) = UnixStream::pair()
        .map_err(|_error| CliError::unavailable("cannot create auth response socket"))?;
    let mut child = Command::new(runner_path()?)
        .stdin(socket_stdio(runner_input))
        .stdout(socket_stdio(runner_output))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_error| CliError::unavailable("cannot start auth runner"))?;
    let encoded = serde_json::to_string(&frame)
        .map_err(|_error| CliError::unavailable("cannot encode auth request"))?;
    request
        .write_all(encoded.as_bytes())
        .and_then(|()| request.write_all(b"\n"))
        .map_err(|_error| CliError::unavailable("cannot write auth request"))?;
    let result = read_result(response, request_id, progress);
    let status = child
        .wait()
        .map_err(|_error| CliError::unavailable("cannot wait for auth runner"))?;
    if !status.success() {
        return Err(CliError::unavailable("authentication runner failed"));
    }
    result
}

fn read_result(
    stream: UnixStream,
    request_id: &str,
    mut progress: impl FnMut(Option<&str>) -> Result<(), CliError>,
) -> Result<(), CliError> {
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|_error| CliError::unavailable("cannot read auth result"))?;
        if line.is_empty() || line.len() > MAX_AUTH_RESPONSE_BYTES {
            return Err(CliError::unavailable("invalid auth runner response"));
        }
        let frame =
            serde_json::from_str::<cortexfs::AuthWireFrame<cortexfs::AuthWireResponse>>(&line)
                .map_err(|_error| CliError::unavailable("invalid auth runner response"))?;
        if frame.abi != cortexfs::AUTH_SOCKET_ABI {
            return Err(CliError::unavailable("invalid auth runner response"));
        }
        match frame.frame {
            cortexfs::AuthWireResponse::Progress { detail, .. } => progress(detail.as_deref())?,
            cortexfs::AuthWireResponse::Result {
                request_id: id, ok, ..
            } if id == request_id => {
                return ok.then_some(()).ok_or_else(|| {
                    CliError::unavailable("authentication profile store unavailable")
                });
            }
            cortexfs::AuthWireResponse::Result { .. } => {
                return Err(CliError::unavailable("invalid auth runner response"));
            }
        }
    }
}

pub(super) fn runner_path() -> Result<PathBuf, CliError> {
    let current = env::current_exe()
        .map_err(|_error| CliError::unavailable("cannot locate ctx executable"))?;
    let parent = current
        .parent()
        .ok_or_else(|| CliError::unavailable("cannot locate auth runner"))?;
    let runner = parent.join("cortexfs-auth-runner");
    fs::symlink_metadata(&runner)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .then_some(runner)
        .ok_or_else(|| CliError::unavailable("cortexfs-auth-runner is not installed"))
}

pub(super) fn socket_stdio(stream: UnixStream) -> Stdio {
    let descriptor: OwnedFd = stream.into();
    Stdio::from(File::from(descriptor))
}
