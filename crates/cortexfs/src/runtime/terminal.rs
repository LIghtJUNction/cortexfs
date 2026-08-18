//! Durable terminal-resource metadata beneath an existing `CortexFS` session.

use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::authority::helpers::atomic_replace_text_with_mode;
use crate::support::plain::{CreatePlainDirMessages, create_plain_dir_with};

pub mod event;

pub use event::{TerminalEvent, append_event, mark_state, next_sequence};

#[cfg(test)]
mod tests;

/// Failure while creating or reading a terminal resource.
#[derive(Debug, thiserror::Error)]
pub enum TerminalResourceError {
    /// The resource id is not a valid `CortexFS` object name.
    #[error("invalid terminal id")]
    InvalidId,
    /// Durable metadata or layout I/O failed.
    #[error("terminal resource I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Durable metadata was not valid JSON.
    #[error("invalid terminal metadata: {0}")]
    Json(#[from] serde_json::Error),
}

/// Durable identity and launch summary for one terminal resource.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalRecord {
    /// Stable terminal resource id.
    pub id: String,
    /// Agent that created or first attached the terminal.
    pub agent: String,
    /// Durable session containing the resource.
    pub session: String,
    /// Linux owner uid represented as text for ABI consistency.
    pub owner: String,
    /// Terminal working directory inside the sandbox.
    pub cwd: String,
    /// Initial command argv, stored as one JSON array.
    pub command: Vec<String>,
    /// created, running, exited, or error.
    pub state: String,
    /// Optional live transport path; this is a locator, not identity.
    pub socket: Option<String>,
    /// Creation timestamp in Unix seconds.
    pub created_at: u64,
}

/// Returns the stable id used by the agent-session compatibility terminal.
#[must_use]
pub fn terminal_id(agent: &str, session: &str) -> String {
    format!("terminal-{agent}-{session}")
}

/// Returns the durable directory for one terminal id below a session.
pub fn resource_dir(session_dir: &Path, id: &str) -> Result<PathBuf, TerminalResourceError> {
    if !crate::is_object_name(id) {
        return Err(TerminalResourceError::InvalidId);
    }
    Ok(session_dir.join("terminal").join(id))
}

/// Creates the terminal resource directory and its inspectable metadata files.
pub fn ensure_layout(
    session_dir: &Path,
    record: &TerminalRecord,
) -> Result<PathBuf, TerminalResourceError> {
    let directory = resource_dir(session_dir, &record.id)?;
    let messages = CreatePlainDirMessages::library_defaults();
    create_plain_dir_with(&directory, messages)?;
    let metadata = format!("{}\n", serde_json::to_string_pretty(record)?);
    atomic_replace_text_with_mode(&directory.join("meta.json"), &metadata, 0o600)?;
    for name in ["state", "status"] {
        atomic_replace_text_with_mode(
            &directory.join(name),
            &format!("{}\n", record.state),
            0o600,
        )?;
    }
    atomic_replace_text_with_mode(
        &directory.join("owner"),
        &format!("{}\n", record.owner),
        0o600,
    )?;
    atomic_replace_text_with_mode(&directory.join("cwd"), &format!("{}\n", record.cwd), 0o600)?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .mode(0o600)
        .open(directory.join("events.jsonl"))?;
    Ok(directory)
}

/// Reads a terminal record from a resource directory.
pub fn read_record(directory: &Path) -> Result<TerminalRecord, TerminalResourceError> {
    let content =
        crate::support::plain::read_small_text_file(&directory.join("meta.json"), 64 * 1024)?;
    Ok(serde_json::from_str(&content)?)
}
