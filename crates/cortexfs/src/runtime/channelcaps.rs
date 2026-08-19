use std::path::{Path, PathBuf};

use crate::runtime::types::ChannelRuntimeError;
use crate::support::plain::{open_plain_directory, path_metadata_no_follow, read_small_text_file};

const MAX_BYTES: u64 = 8 * 1024;

pub(super) fn add_dir(dirs: &mut Vec<PathBuf>, path: &Path) -> Result<(), ChannelRuntimeError> {
    match path_metadata_no_follow(path) {
        Ok(metadata) if metadata.is_dir() => {
            open_plain_directory(path).map_err(|_error| ChannelRuntimeError::InvalidDirectory)?;
            dirs.push(path.to_path_buf());
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) | Err(_) => Err(ChannelRuntimeError::InvalidDirectory),
    }
}

pub(super) fn read_caps(
    source: &Path,
    uid: u32,
    channel: &str,
) -> Result<Vec<String>, ChannelRuntimeError> {
    let paths = [
        cortexfs_paths::home_channel_control_file_path(source, &uid.to_string(), channel, "cap"),
        cortexfs_paths::channel_control_file_path(source, channel, "cap"),
    ];
    let Some(path) = paths
        .iter()
        .find(|path| path_metadata_no_follow(path).is_ok())
    else {
        return Ok(Vec::new());
    };
    let content = read_small_text_file(path, MAX_BYTES)
        .map_err(|_error| ChannelRuntimeError::CannotReadCapability)?;
    let caps = content
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    caps.iter()
        .all(|cap| {
            !cap.is_empty() && cap.len() <= 128 && !cap.bytes().any(|byte| byte.is_ascii_control())
        })
        .then_some(caps)
        .ok_or(ChannelRuntimeError::InvalidCapability)
}

pub(super) fn tool_name(hit: &crate::ToolHit) -> Option<String> {
    hit.path().file_name()?.to_str().map(ToOwned::to_owned)
}
