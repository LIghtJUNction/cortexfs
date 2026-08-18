use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::{RuntimeClientError, interaction};

use super::{MAX_SESSION_FRAME_BYTES, MAX_SESSION_RESPONSE_BYTES};

pub(super) fn send_json_stream<F, E>(socket: &Path, frame: &str, mut on_frame: F) -> Result<(), E>
where
    F: FnMut(&str) -> Result<(), E>,
    E: From<RuntimeClientError>,
{
    send_json_stream_with(socket, frame, |_stream, line| on_frame(line))
}

pub(super) fn send_json_stream_with<F, E>(
    socket: &Path,
    frame: &str,
    mut on_frame: F,
) -> Result<(), E>
where
    F: FnMut(&mut UnixStream, &str) -> Result<(), E>,
    E: From<RuntimeClientError>,
{
    if frame.len().saturating_add(1) > MAX_SESSION_FRAME_BYTES {
        return Err(E::from(RuntimeClientError::InvalidRequest));
    }
    let mut stream =
        UnixStream::connect(socket).map_err(|_error| E::from(RuntimeClientError::CannotConnect))?;
    stream
        .write_all(frame.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|_error| E::from(RuntimeClientError::CannotWrite))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|_error| E::from(RuntimeClientError::CannotRead))?;
    let reader = stream
        .try_clone()
        .map_err(|_error| E::from(RuntimeClientError::CannotRead))?;
    let mut reader = BufReader::new(reader).take(MAX_SESSION_RESPONSE_BYTES + 1);
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
        on_frame(&mut stream, text)?;
        frames = frames.saturating_add(1);
    }
    if frames == 0 {
        return Err(E::from(RuntimeClientError::InvalidFrame));
    }
    Ok(())
}

pub(super) fn write_interaction_request(
    stream: &mut UnixStream,
    request: interaction::InteractionRequest,
) -> Result<(), RuntimeClientError> {
    let frame = interaction::InteractionFrame::request(request)
        .encode()
        .map_err(|_error| RuntimeClientError::InvalidRequest)?;
    if frame.len() > MAX_SESSION_FRAME_BYTES {
        return Err(RuntimeClientError::InvalidRequest);
    }
    stream
        .write_all(&frame)
        .and_then(|()| stream.flush())
        .map_err(|_error| RuntimeClientError::CannotWrite)
}
