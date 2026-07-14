use super::*;

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
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                buffer.push(byte[0]);
                if buffer.len() > MAX_SOCKET_FRAME_BYTES {
                    return Err(SocketRuntimeError::Request(
                        SocketRequestError::FrameTooLarge {
                            bytes: buffer.len(),
                        },
                    ));
                }
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(_error) => return Err(SocketRuntimeError::CannotReadFrame),
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

#[derive(Clone, Copy)]
pub(crate) struct SocketDebugTiming {
    pub(crate) start_unix_ms: u128,
    pub(crate) request_start_unix_ms: Option<u128>,
}

impl SocketDebugTiming {
    pub(crate) fn with_request_baseline(mut self) -> Self {
        self.request_start_unix_ms = Some(current_unix_millis());
        self
    }
}

pub(crate) fn socket_debug_timing_from_frame(frame: &str) -> Option<SocketDebugTiming> {
    let value = serde_json::from_str::<Value>(frame).ok()?;
    if value.get("debug").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    Some(SocketDebugTiming {
        start_unix_ms: current_unix_millis(),
        request_start_unix_ms: None,
    })
}

pub(crate) fn current_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

pub(crate) fn write_socket_debug_timing_frame(
    stream: &mut UnixStream,
    timing: SocketDebugTiming,
    stage: &str,
) -> Result<(), SocketRuntimeError> {
    let elapsed_ms = current_unix_millis().saturating_sub(timing.start_unix_ms);
    let mut frame = serde_json::json!({
        "type": "debug",
        "stage": stage,
        "elapsed_ms": elapsed_ms
    });
    if let Some(request_start_unix_ms) = timing.request_start_unix_ms
        && let Some(object) = frame.as_object_mut()
    {
        object.insert(
            "request_elapsed_ms".to_owned(),
            serde_json::json!(current_unix_millis().saturating_sub(request_start_unix_ms)),
        );
    }
    write_socket_frame(stream, &frame.to_string())
}

pub(crate) fn write_optional_socket_debug_timing_frame(
    stream: &mut UnixStream,
    timing: Option<SocketDebugTiming>,
    stage: &str,
) -> Result<(), SocketRuntimeError> {
    if let Some(timing) = timing {
        write_socket_debug_timing_frame(stream, timing, stage)?;
    }
    Ok(())
}

pub(crate) fn apply_socket_debug_timing_env(
    command: &mut Command,
    timing: Option<SocketDebugTiming>,
) {
    if let Some(timing) = timing {
        command.env("CTX_AGENT_DEBUG_TIMING", "1").env(
            "CTX_AGENT_DEBUG_START_UNIX_MS",
            timing.start_unix_ms.to_string(),
        );
    }
}

pub(crate) fn is_socket_debug_timing_frame(frame: &str, timing: Option<SocketDebugTiming>) -> bool {
    if timing.is_none() {
        return false;
    }
    let Ok(value) = serde_json::from_str::<Value>(frame) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.len() != 3 {
        return false;
    }
    if value.get("type").and_then(Value::as_str) != Some("debug") {
        return false;
    }
    if value.get("elapsed_ms").and_then(Value::as_u64).is_none() {
        return false;
    }
    matches!(
        value.get("stage").and_then(Value::as_str),
        Some("agent_runner_ready" | "model_spawn_start" | "model_spawned" | "first_model_frame")
    )
}
