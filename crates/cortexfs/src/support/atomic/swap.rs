use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use super::write::publish_at;
use super::{AtomicCommit, AtomicMetadata, AtomicReplaceOutcome};
use crate::support::plain::{open_plain_directory, plain_file_name};

pub fn atomic_replace_text_preserving_metadata(path: &Path, content: &str) -> std::io::Result<()> {
    preserving(path, content, None, None)
}

pub fn atomic_replace_text_preserving_metadata_if_matches(
    path: &Path,
    content: &str,
    expected: (u64, u64),
) -> std::io::Result<()> {
    preserving(path, content, Some(expected), None)
}

pub(super) fn preserving(
    path: &Path,
    content: &str,
    expected: Option<(u64, u64)>,
    before_commit: Option<&mut dyn FnMut() -> std::io::Result<()>>,
) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_plain_directory(parent)?;
    let name = plain_file_name(path)?;
    let fd = nix::fcntl::openat(
        &parent_dir,
        name,
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_NONBLOCK
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )?;
    let existing = fs::File::from(fd);
    let metadata = existing.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other(
            "atomic replace target is not a regular file",
        ));
    }
    let identity = (metadata.dev(), metadata.ino());
    if expected.is_some_and(|value| value != identity) {
        return Err(std::io::Error::other("atomic replace target changed"));
    }
    let replacement = AtomicMetadata {
        mode: metadata.permissions().mode() & 0o7777,
        exact_mode: true,
        owner: Some((metadata.uid(), metadata.gid())),
        identity: Some(expected.unwrap_or(identity)),
        commit: AtomicCommit::Replace,
    };
    drop(existing);
    publish_at(
        &parent_dir,
        name,
        content.as_bytes(),
        &replacement,
        before_commit,
    )
    .and_then(AtomicReplaceOutcome::into_result)
}
