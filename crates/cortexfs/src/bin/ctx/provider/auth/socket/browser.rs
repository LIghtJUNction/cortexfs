use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

use crate::*;

#[expect(
    clippy::redundant_pub_crate,
    reason = "provider OAuth dispatch reaches socketpair client"
)]
pub(crate) fn oauth_browser_login(
    provider: &str,
    profile: &str,
    config: &CtxProviderConfig,
    oauth: cortexfs::OAuthProviderConfig,
    timeout_secs: u64,
) -> Result<(), CliError> {
    let (mut request, input) = UnixStream::pair()
        .map_err(|_error| CliError::unavailable("cannot create auth request socket"))?;
    let (output, response) = UnixStream::pair()
        .map_err(|_error| CliError::unavailable("cannot create auth response socket"))?;
    let mut child = Command::new(super::runner_path()?)
        .stdin(super::socket_stdio(input))
        .stdout(super::socket_stdio(output))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_error| CliError::unavailable("cannot start auth runner"))?;
    let request_id = format!("auth-{}", hex_bytes(&read_system_entropy(16)?));
    let frame = cortexfs::AuthWireFrame::new(cortexfs::AuthWireRequest::Browser {
        request_id: request_id.clone(),
        provider: provider.to_owned(),
        profile: profile.to_owned(),
        base_url: config.base_url.clone(),
        methods: config.auth_methods(),
        oauth: Box::new(oauth),
        timeout_secs,
    });
    let encoded = serde_json::to_string(&frame)
        .map_err(|_error| CliError::unavailable("cannot encode auth request"))?;
    request
        .write_all(encoded.as_bytes())
        .and_then(|()| request.write_all(b"\n"))
        .map_err(|_error| CliError::unavailable("cannot write auth request"))?;
    let mut reader = BufReader::new(response);
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|_error| CliError::unavailable("cannot read auth result"))?;
        let frame =
            serde_json::from_str::<cortexfs::AuthWireFrame<cortexfs::AuthWireResponse>>(&line)
                .map_err(|_error| CliError::unavailable("invalid auth runner response"))?;
        match frame.frame {
            cortexfs::AuthWireResponse::Progress {
                detail: Some(url), ..
            } => print_line(&url)?,
            cortexfs::AuthWireResponse::Progress { .. } => {}
            cortexfs::AuthWireResponse::Result {
                request_id: id, ok, ..
            } if id == request_id => {
                let status = child
                    .wait()
                    .map_err(|_error| CliError::unavailable("cannot wait for auth runner"))?;
                return (status.success() && ok)
                    .then_some(())
                    .ok_or_else(|| CliError::unavailable("authentication failed"));
            }
            cortexfs::AuthWireResponse::Result { .. } => {
                return Err(CliError::unavailable("invalid auth runner response"));
            }
        }
    }
}
