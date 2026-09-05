use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{RuntimeClientError, read_frame};

const MAX_STATUS_FRAME_BYTES: u64 = 256 * 1024;

/// Typed, non-secret status returned by an Agent session socket.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeStatus {
    #[serde(rename = "type")]
    pub kind: String,
    pub session: String,
    pub status: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub step: u32,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub context_revision: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Queries one session without opening or mutating durable history.
pub fn status(socket: &Path, session: &str) -> Result<RuntimeStatus, RuntimeClientError> {
    if session.is_empty() || session.contains('\0') {
        return Err(RuntimeClientError::InvalidRequest);
    }
    let mut stream =
        UnixStream::connect(socket).map_err(|_error| RuntimeClientError::CannotConnect)?;
    let frame = json!({"op": "status", "session": session}).to_string();
    stream
        .write_all(frame.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .map_err(|_error| RuntimeClientError::CannotWrite)?;
    let value: Value = read_frame(stream, MAX_STATUS_FRAME_BYTES, Duration::from_secs(5))?;
    if value.get("type").and_then(Value::as_str) == Some("error") {
        return Err(RuntimeClientError::Rejected(
            value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("EIO")
                .to_owned(),
        ));
    }
    serde_json::from_value(value).map_err(|_error| RuntimeClientError::InvalidFrame)
}
