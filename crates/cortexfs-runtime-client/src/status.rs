use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::RuntimeClientError;

const MAX_STATUS_FRAME_BYTES: u64 = 256 * 1024;

/// Typed, non-secret status returned by an agent session socket.
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
    let frame = json!({"op": "status", "session": session}).to_string();
    if u64::try_from(frame.len()).unwrap_or(u64::MAX) >= MAX_STATUS_FRAME_BYTES {
        return Err(RuntimeClientError::InvalidRequest);
    }
    let mut stream =
        UnixStream::connect(socket).map_err(|_error| RuntimeClientError::CannotConnect)?;
    stream
        .write_all(frame.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .map_err(|_error| RuntimeClientError::CannotWrite)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    let mut bytes = Vec::new();
    BufReader::new(stream)
        .take(MAX_STATUS_FRAME_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    if bytes.is_empty()
        || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_STATUS_FRAME_BYTES
        || bytes.last() != Some(&b'\n')
    {
        return Err(RuntimeClientError::InvalidFrame);
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_error| RuntimeClientError::InvalidFrame)?;
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
