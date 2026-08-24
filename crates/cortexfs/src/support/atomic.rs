//! Symlink-safe atomic publication for stable control files.

use std::path::Path;

mod commit;
mod name;
mod publish;
mod swap;
mod write;

pub use name::{generated_sibling_name, generated_sibling_target};
pub use publish::{
    atomic_create_text_with_mode, atomic_replace_text_with_mode, write_file_atomic_at,
};
pub use swap::{
    atomic_replace_text_preserving_metadata, atomic_replace_text_preserving_metadata_if_matches,
};

#[derive(Clone, Copy)]
pub(crate) enum AtomicCommit {
    Replace,
    NoReplace,
}

#[derive(Debug)]
pub(crate) enum AtomicReplaceOutcome {
    Synced,
    PublishedUnsynced(std::io::Error),
}

impl AtomicReplaceOutcome {
    fn into_result(self) -> std::io::Result<()> {
        match self {
            Self::Synced => Ok(()),
            Self::PublishedUnsynced(error) => Err(error),
        }
    }
}

struct AtomicMetadata {
    mode: u32,
    exact_mode: bool,
    owner: Option<(u32, u32)>,
    identity: Option<(u64, u64)>,
    commit: AtomicCommit,
}

pub(crate) fn atomic_replace_text(path: &Path, content: &str) -> std::io::Result<()> {
    atomic_replace_text_with_mode(path, content, 0o600)
}

pub(crate) fn atomic_replace_text_outcome(
    path: &Path,
    content: &str,
) -> std::io::Result<AtomicReplaceOutcome> {
    write::publish_path(
        path,
        content.as_bytes(),
        &metadata(0o600, true, AtomicCommit::Replace),
        None,
    )
}

pub(crate) fn atomic_write_owned(
    path: &Path,
    content: &str,
    mode: u32,
    owner: (u32, u32),
    commit: AtomicCommit,
) -> std::io::Result<()> {
    let metadata = AtomicMetadata {
        mode,
        exact_mode: true,
        owner: Some(owner),
        identity: None,
        commit,
    };
    write::publish_path(path, content.as_bytes(), &metadata, None)
        .and_then(AtomicReplaceOutcome::into_result)
}

fn publish_outcome(result: std::io::Result<()>) -> AtomicReplaceOutcome {
    result.map_or_else(AtomicReplaceOutcome::PublishedUnsynced, |()| {
        AtomicReplaceOutcome::Synced
    })
}

const fn metadata(mode: u32, exact_mode: bool, commit: AtomicCommit) -> AtomicMetadata {
    AtomicMetadata {
        mode,
        exact_mode,
        owner: None,
        identity: None,
        commit,
    }
}

#[cfg(test)]
pub(crate) fn atomic_replace_text_preserving_metadata_with_hook(
    path: &Path,
    content: &str,
    before_commit: &mut dyn FnMut() -> std::io::Result<()>,
) -> std::io::Result<()> {
    swap::preserving(path, content, None, Some(before_commit))
}
