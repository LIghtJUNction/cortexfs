use std::fs;
use std::io::Write;
use std::path::Path;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::support::atomic::atomic_replace_text_with_mode;

/// One ordered, replayable terminal fact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalEvent {
    /// Monotonic sequence within one terminal resource.
    pub seq: u64,
    /// Unix timestamp in seconds.
    pub ts: u64,
    /// Event discriminator, for example pty.output or process.exit.
    #[serde(rename = "type")]
    pub kind: String,
    /// Base64 payload for byte-oriented PTY output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_b64: Option<String>,
    /// Optional process exit code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
}

impl TerminalEvent {
    /// Builds an event containing raw PTY bytes without assuming UTF-8.
    #[must_use]
    pub fn output(seq: u64, ts: u64, bytes: &[u8]) -> Self {
        Self {
            seq,
            ts,
            kind: "pty.output".to_owned(),
            data_b64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            exit_code: None,
        }
    }

    /// Builds the terminal process exit fact.
    #[must_use]
    pub fn exit(seq: u64, ts: u64, code: u32) -> Self {
        Self {
            seq,
            ts,
            kind: "process.exit".to_owned(),
            data_b64: None,
            exit_code: Some(code),
        }
    }
}

/// Appends one complete event line and syncs the file and parent directory.
pub fn append_event(path: &Path, event: &TerminalEvent) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let parent_file = crate::support::plain::open_plain_directory(parent)?;
    let fd = nix::fcntl::openat(
        &parent_file,
        name,
        nix::fcntl::OFlag::O_APPEND
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .map_err(std::io::Error::from)?;
    let mut file = fs::File::from(fd);
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other(
            "terminal event path is not a regular file",
        ));
    }
    let mut line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    file.sync_all()?;
    parent_file.sync_all()
}

/// Returns the next sequence number without resetting an existing resource.
pub fn next_sequence(path: &Path) -> std::io::Result<u64> {
    let content = match crate::support::plain::read_small_text_file(path, 16 * 1024 * 1024) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(1),
        Err(error) => return Err(error),
    };
    content
        .lines()
        .map(|line| serde_json::from_str::<TerminalEvent>(line).map_err(std::io::Error::other))
        .try_fold(0, |max, event| event.map(|event| max.max(event.seq)))
        .map(|max| max.saturating_add(1))
}

/// Updates durable state projections after the PTY process exits.
pub fn mark_state(events_path: &Path, state: &str) -> std::io::Result<()> {
    let directory = events_path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let mut record = super::read_record(directory).map_err(std::io::Error::other)?;
    state.clone_into(&mut record.state);
    let metadata = format!(
        "{}\n",
        serde_json::to_string_pretty(&record).map_err(std::io::Error::other)?
    );
    atomic_replace_text_with_mode(&directory.join("meta.json"), &metadata, 0o600)?;
    for name in ["state", "status"] {
        atomic_replace_text_with_mode(&directory.join(name), &format!("{state}\n"), 0o600)?;
    }
    Ok(())
}
