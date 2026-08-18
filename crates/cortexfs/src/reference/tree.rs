use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use nix::libc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::support::plain::{open_directory_at, proc_fd_path};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeEntry {
    pub(crate) path: String,
    pub(crate) kind: String,
    pub(crate) digest: String,
    pub(crate) mode: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

pub fn collect_tree(root: &File, excluded: &str) -> io::Result<Vec<TreeEntry>> {
    let mut entries = Vec::new();
    collect(root, Path::new(""), excluded, &mut entries)?;
    entries.sort();
    Ok(entries)
}

fn collect(
    directory: &File,
    prefix: &Path,
    excluded: &str,
    entries: &mut Vec<TreeEntry>,
) -> io::Result<()> {
    for entry in fs::read_dir(proc_fd_path(directory))? {
        let name = entry?.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        let path = prefix.join(name);
        let relative = path
            .to_str()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        if relative == excluded {
            continue;
        }
        let stat =
            nix::sys::stat::fstatat(directory, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(io::Error::from)?;
        let kind = stat.st_mode & libc::S_IFMT;
        if kind == libc::S_IFDIR {
            entries.push(record(relative, "directory", String::new(), &stat));
            let child = open_directory_at(directory, name.as_ref())?;
            collect(&child, &path, excluded, entries)?;
        } else if kind == libc::S_IFREG {
            let mut file = open_file(directory, name)?;
            let mut bytes = Vec::new();
            #[expect(
                clippy::verbose_file_reads,
                reason = "the nofollow file descriptor must remain authoritative during the read"
            )]
            file.read_to_end(&mut bytes)?;
            entries.push(record(relative, "file", digest_hex(&bytes), &stat));
        } else {
            return Err(io::Error::other("provider tree contains unsupported entry"));
        }
    }
    Ok(())
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ignored = write!(output, "{byte:02x}");
            output
        })
}

fn open_file(parent: &File, name: &str) -> io::Result<File> {
    nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(File::from)
    .map_err(io::Error::from)
}

fn record(path: &str, kind: &str, digest: String, stat: &libc::stat) -> TreeEntry {
    TreeEntry {
        path: path.to_owned(),
        kind: kind.to_owned(),
        digest,
        mode: stat.st_mode & 0o7777,
        uid: stat.st_uid,
        gid: stat.st_gid,
    }
}
