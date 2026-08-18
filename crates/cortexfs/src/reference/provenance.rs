use std::fs::File;
use std::io;

use nix::libc;
use serde::{Deserialize, Serialize};

use super::tree::{TreeEntry, collect_tree};
use crate::support::plain::{open_directory_at, read_small_text_file_at, write_text_file_at};
use crate::support::receipt::{EntryKind, receipt_at};

pub const MANIFEST: &str = ".cortexfs-provider.json";
const SCHEMA: &str = "cortexfs.provider-projection/v1";
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    entries: Vec<TreeEntry>,
}

pub fn seal(directory: &File) -> io::Result<()> {
    let manifest = Manifest {
        schema: SCHEMA.to_owned(),
        entries: collect_tree(directory, MANIFEST)?,
    };
    let content = serde_json::to_string(&manifest)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        + "\n";
    write_text_file_at(directory, MANIFEST, &content, 0o600)?;
    directory.sync_all()
}

pub fn verify(directory: &File) -> io::Result<()> {
    let content = read_small_text_file_at(directory, MANIFEST, MAX_BYTES, "invalid manifest")?;
    let manifest: Manifest = serde_json::from_str(&content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if manifest.schema != SCHEMA || manifest.entries != collect_tree(directory, MANIFEST)? {
        return Err(io::Error::other("provider provenance mismatch"));
    }
    let stat = nix::sys::stat::fstatat(
        directory,
        MANIFEST,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io::Error::from)?;
    let owner = (
        nix::unistd::getuid().as_raw(),
        nix::unistd::getgid().as_raw(),
    );
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG
        || stat.st_mode & 0o7777 != 0o600
        || (stat.st_uid, stat.st_gid) != owner
    {
        return Err(io::Error::other("provider manifest metadata mismatch"));
    }
    Ok(())
}

pub fn has_manifest(directory: &File) -> io::Result<bool> {
    match receipt_at(directory, MANIFEST, EntryKind::File) {
        Ok(Some(_)) => Ok(true),
        Ok(None) => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn legacy_matches(existing: &File, desired: &File) -> io::Result<bool> {
    Ok(collect_tree(existing, MANIFEST)? == collect_tree(desired, MANIFEST)?)
}

pub fn accept_old(stage: &File, desired: Option<&File>, active: bool) -> io::Result<bool> {
    let old = open_directory_at(stage, "old".as_ref())?;
    match receipt_at(&old, MANIFEST, EntryKind::File) {
        Ok(Some(_)) => Ok(verify(&old).is_ok()),
        Ok(None) => match (active, desired) {
            (true, Some(desired)) => legacy_matches(&old, desired),
            _ => Ok(false),
        },
        Err(_) => Ok(false),
    }
}
