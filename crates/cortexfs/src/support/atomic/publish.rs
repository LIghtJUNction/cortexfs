use std::{fs, path::Path};

use super::write::{publish_at, publish_path};
use super::{AtomicCommit, AtomicMetadata, AtomicReplaceOutcome, metadata};

pub fn atomic_replace_text_with_mode(path: &Path, content: &str, mode: u32) -> std::io::Result<()> {
    let metadata = metadata(mode, true, AtomicCommit::Replace);
    publish_text(path, content, &metadata)
}

pub fn atomic_create_text_with_mode(path: &Path, content: &str, mode: u32) -> std::io::Result<()> {
    let metadata = metadata(mode, true, AtomicCommit::NoReplace);
    publish_text(path, content, &metadata)
}

/// Atomically publishes one new plain file relative to a held directory fd.
pub fn write_file_atomic_at(
    parent: &fs::File,
    name: &str,
    content: &[u8],
    mode: u32,
) -> std::io::Result<()> {
    let metadata = metadata(mode, false, AtomicCommit::NoReplace);
    publish_at(parent, name, content, &metadata, None).and_then(AtomicReplaceOutcome::into_result)
}

fn publish_text(path: &Path, content: &str, metadata: &AtomicMetadata) -> std::io::Result<()> {
    publish_path(path, content.as_bytes(), metadata, None)
        .and_then(AtomicReplaceOutcome::into_result)
}
