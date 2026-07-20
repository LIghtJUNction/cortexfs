use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

use super::SOCKET_REQUEST_READ_TIMEOUT;
use crate::{
    MAX_SOCKET_FRAME_BYTES, SocketRequestError, SocketRuntimeError, SocketRuntimeResponse,
};

mod timing;
pub(super) use timing::{
    apply_socket_debug_timing_env, is_socket_debug_timing_frame, socket_debug_timing_from_frame,
    write_optional_socket_debug_timing_frame,
};

const SOCKET_READ_CHUNK_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct SocketDebugTiming {
    pub(super) start_unix_ms: u128,
    request_start_unix_ms: Option<u128>,
}

impl SocketDebugTiming {
    pub(in crate::runtime::socket) fn with_request_baseline(mut self) -> Self {
        self.request_start_unix_ms = Some(timing::current_unix_millis());
        self
    }
}

pub(crate) fn read_socket_request_frame_from_stream(
    stream: &mut UnixStream,
) -> Result<String, SocketRuntimeError> {
    let restore_blocking = stream
        .read_timeout()
        .map_err(|_error| SocketRuntimeError::CannotReadFrame)?
        .is_none();
    if restore_blocking {
        stream
            .set_read_timeout(Some(SOCKET_REQUEST_READ_TIMEOUT))
            .map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
    }
    let frame = read_socket_request_frame_body(stream);
    if restore_blocking {
        stream
            .set_read_timeout(None)
            .map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
    }
    frame
}

pub(crate) fn read_socket_request_frame_body(
    stream: &mut UnixStream,
) -> Result<String, SocketRuntimeError> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; SOCKET_READ_CHUNK_BYTES];
    loop {
        let peeked = nix::sys::socket::recv(
            stream.as_raw_fd(),
            &mut chunk,
            nix::sys::socket::MsgFlags::MSG_PEEK,
        )
        .map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
        if peeked == 0 {
            break;
        }
        let peeked_chunk = chunk
            .get(..peeked)
            .ok_or(SocketRuntimeError::CannotReadFrame)?;
        let read = peeked_chunk
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(peeked, |index| index + 1)
            .min(
                MAX_SOCKET_FRAME_BYTES
                    .saturating_add(1)
                    .saturating_sub(buffer.len()),
            );
        let consumed = chunk
            .get_mut(..read)
            .ok_or(SocketRuntimeError::CannotReadFrame)?;
        stream
            .read_exact(consumed)
            .map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
        buffer.extend_from_slice(consumed);
        if buffer.len() > MAX_SOCKET_FRAME_BYTES {
            return Err(SocketRuntimeError::Request(
                SocketRequestError::FrameTooLarge {
                    bytes: buffer.len(),
                },
            ));
        }
        if buffer.last() == Some(&b'\n') {
            break;
        }
    }
    String::from_utf8(buffer)
        .map_err(|_error| SocketRuntimeError::Request(SocketRequestError::InvalidJson))
}

pub(crate) fn write_socket_runtime_response(
    stream: &mut UnixStream,
    response: &SocketRuntimeResponse,
) -> Result<(), SocketRuntimeError> {
    stream
        .write_all(response.jsonl().as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|_error| SocketRuntimeError::CannotWriteResponse)
}

pub(crate) fn write_socket_frame(
    stream: &mut UnixStream,
    frame: &str,
) -> Result<(), SocketRuntimeError> {
    stream
        .write_all(frame.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|_error| SocketRuntimeError::CannotWriteResponse)
}
