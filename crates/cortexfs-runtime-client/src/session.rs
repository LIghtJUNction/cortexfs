use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::json;

use crate::RuntimeClientError;

const MAX_SESSION_FRAME_BYTES: usize = 256 * 1024;
const MAX_SESSION_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

/// Stable `send` request fields for an agent session socket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionSendRequest<'a> {
    pub request_id: &'a str,
    pub session: &'a str,
    pub scope: &'a str,
    pub cwd: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub input: &'a str,
}

/// Sends one session request and returns the runtime's canonical JSONL events.
pub fn send(
    socket: &Path,
    request: SessionSendRequest<'_>,
) -> Result<Vec<String>, RuntimeClientError> {
    validate(&request)?;
    let frame = json!({
        "op": "send",
        "id": request.request_id,
        "session": request.session,
        "scope": request.scope,
        "cwd": request.cwd,
        "workspace": request.workspace,
        "input": request.input,
    })
    .to_string();
    if frame.len() > MAX_SESSION_FRAME_BYTES {
        return Err(RuntimeClientError::InvalidRequest);
    }
    let mut stream =
        UnixStream::connect(socket).map_err(|_error| RuntimeClientError::CannotConnect)?;
    stream
        .write_all(frame.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .map_err(|_error| RuntimeClientError::CannotWrite)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    let mut bytes = Vec::new();
    stream
        .take(MAX_SESSION_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    if bytes.is_empty()
        || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SESSION_RESPONSE_BYTES
        || bytes.last() != Some(&b'\n')
    {
        return Err(RuntimeClientError::InvalidFrame);
    }
    String::from_utf8(bytes)
        .map(|text| text.lines().map(str::to_owned).collect())
        .map_err(|_error| RuntimeClientError::InvalidFrame)
}

fn validate(request: &SessionSendRequest<'_>) -> Result<(), RuntimeClientError> {
    if request.scope != "private" && request.scope != "shared" && request.scope != "temp" {
        return Err(RuntimeClientError::InvalidRequest);
    }
    let fields = [
        request.request_id,
        request.session,
        request.scope,
        request.input,
    ];
    if fields
        .iter()
        .any(|field| field.is_empty() || field.contains('\0'))
        || request.cwd.is_some_and(|value| value.contains('\0'))
        || request.workspace.is_some_and(|value| value.contains('\0'))
    {
        return Err(RuntimeClientError::InvalidRequest);
    }
    Ok(())
}
