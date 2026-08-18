use std::os::unix::net::UnixStream;

use super::super::write_socket_frame;
use super::{AgentProcessOutcome, SocketRuntimeError};

pub(super) fn deliver_host_frame(
    stream: &mut UnixStream,
    frame: &str,
) -> Result<(), SocketRuntimeError> {
    match write_socket_frame(stream, frame) {
        Ok(()) | Err(SocketRuntimeError::CannotWriteResponse) => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn deliver_terminal_batch(
    stream: &mut UnixStream,
    frames: &[String],
    process: AgentProcessOutcome,
) -> Result<(), SocketRuntimeError> {
    let count = match process {
        AgentProcessOutcome::Success => 1,
        AgentProcessOutcome::Error => 2,
        AgentProcessOutcome::Cancelled => 0,
    };
    let start = frames
        .len()
        .checked_sub(count)
        .ok_or(SocketRuntimeError::InvalidAgentOutput)?;
    let terminal = frames
        .get(start..)
        .ok_or(SocketRuntimeError::InvalidAgentOutput)?;
    for frame in terminal {
        deliver_host_frame(stream, frame)?;
    }
    Ok(())
}

pub(super) fn normalize_observation(value: &str) -> (String, bool) {
    const LIMIT: usize = 16 * 1024;
    if value.len() <= LIMIT {
        return (value.to_owned(), false);
    }
    let marker = "\n[truncated]\n";
    let mut end = LIMIT.saturating_sub(marker.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (
        format!("{}{marker}", value.get(..end).unwrap_or_default()),
        true,
    )
}
