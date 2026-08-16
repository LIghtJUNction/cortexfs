use std::io::{BufRead, BufReader, Read, Write};
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
    let mut frames = Vec::new();
    send_stream(socket, request, |frame| {
        frames.push(frame.to_owned());
        Ok::<(), RuntimeClientError>(())
    })?;
    Ok(frames)
}

/// Sends one session request and invokes the callback for each event frame.
pub fn send_stream<F, E>(
    socket: &Path,
    request: SessionSendRequest<'_>,
    mut on_frame: F,
) -> Result<(), E>
where
    F: FnMut(&str) -> Result<(), E>,
    E: From<RuntimeClientError>,
{
    validate(&request).map_err(E::from)?;
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
        return Err(E::from(RuntimeClientError::InvalidRequest));
    }
    let mut stream =
        UnixStream::connect(socket).map_err(|_error| E::from(RuntimeClientError::CannotConnect))?;
    stream
        .write_all(frame.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .map_err(|_error| E::from(RuntimeClientError::CannotWrite))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|_error| E::from(RuntimeClientError::CannotRead))?;
    let mut reader = BufReader::new(stream).take(MAX_SESSION_RESPONSE_BYTES + 1);
    let mut line = Vec::new();
    let mut total = 0_u64;
    let mut frames = 0_u64;
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|_error| E::from(RuntimeClientError::CannotRead))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        if total > MAX_SESSION_RESPONSE_BYTES || line.last() != Some(&b'\n') {
            return Err(E::from(RuntimeClientError::InvalidFrame));
        }
        line.pop();
        let text = String::from_utf8(std::mem::take(&mut line))
            .map_err(|_error| E::from(RuntimeClientError::InvalidFrame))?;
        let text = text.strip_suffix('\r').unwrap_or(&text);
        on_frame(text)?;
        frames = frames.saturating_add(1);
    }
    if frames == 0 {
        return Err(E::from(RuntimeClientError::InvalidFrame));
    }
    Ok(())
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
