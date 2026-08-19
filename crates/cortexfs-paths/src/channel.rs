use std::path::{Path, PathBuf};

/// Returns one global channel instance directory.
#[must_use]
pub fn channel_path(root: &Path, channel: &str) -> PathBuf {
    root.join("channel").join(channel)
}

/// Returns one global channel instance tool directory.
#[must_use]
pub fn channel_tool_path(root: &Path, channel: &str) -> PathBuf {
    channel_path(root, channel).join("tool")
}

/// Returns one global channel instance control directory.
#[must_use]
pub fn channel_control_path(root: &Path, channel: &str) -> PathBuf {
    root.join("channel").join(format!("{channel}.d"))
}

/// Returns one global channel control file.
#[must_use]
pub fn channel_control_file_path(root: &Path, channel: &str, file: &str) -> PathBuf {
    channel_control_path(root, channel).join(file)
}

/// Returns the per-user channel subsystem root.
#[must_use]
pub fn home_channel_root_path(root: &Path, uid: &str) -> PathBuf {
    root.join("home").join(uid).join("channel")
}

/// Returns one per-user channel instance directory.
#[must_use]
pub fn home_channel_path(root: &Path, uid: &str, channel: &str) -> PathBuf {
    home_channel_root_path(root, uid).join(channel)
}

/// Returns one per-user channel instance tool directory.
#[must_use]
pub fn home_channel_tool_path(root: &Path, uid: &str, channel: &str) -> PathBuf {
    home_channel_path(root, uid, channel).join("tool")
}

/// Returns one per-user channel instance control directory.
#[must_use]
pub fn home_channel_control_path(root: &Path, uid: &str, channel: &str) -> PathBuf {
    home_channel_root_path(root, uid).join(format!("{channel}.d"))
}

/// Returns one per-user channel control file.
#[must_use]
pub fn home_channel_control_file_path(
    root: &Path,
    uid: &str,
    channel: &str,
    file: &str,
) -> PathBuf {
    home_channel_control_path(root, uid, channel).join(file)
}
