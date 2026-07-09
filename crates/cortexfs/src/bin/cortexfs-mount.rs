#![forbid(unsafe_code)]
#![expect(
    clippy::allow_attributes,
    reason = "allow target-specific lint exceptions"
)]
#![allow(
    unfulfilled_lint_expectations,
    reason = "expected target-specific lint results"
)]
#![expect(
    clippy::wildcard_imports,
    reason = "uniform submodules with wildcard imports"
)]
#![expect(clippy::redundant_pub_crate, reason = "submodule visibility alignment")]
#![expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "internal structs with scoped fields"
)]
#![expect(clippy::module_inception, reason = "allow submodule self name")]

use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cortexfs::{
    AbiPathKind, FUSE_V1_ROOT_INODE, FuseV1Attr, FuseV1DirEntry, FuseV1Error, FuseV1FileType,
    FuseV1Node, FuseV1Projection, MAX_FUSE_V1_SMALL_READ_BYTES, MAX_FUSE_V1_SMALL_WRITE_BYTES,
    classify_abi_path, is_object_name, parse_abi_path,
};
use fuser::{
    AccessFlags, BsdFileFlags, Config, CopyFileRangeFlags, Errno, FileAttr, FileHandle, FileType,
    Filesystem, FopenFlags, Generation, INodeNo, IoctlFlags, KernelConfig, LockOwner, MountOption,
    OpenAccMode, OpenFlags, PollEvents, PollFlags, PollNotifier, RenameFlags, ReplyAttr, ReplyBmap,
    ReplyCreate, ReplyData, ReplyDirectory, ReplyDirectoryPlus, ReplyEmpty, ReplyEntry, ReplyIoctl,
    ReplyLseek, ReplyOpen, ReplyPoll, ReplyStatfs, ReplyWrite, ReplyXattr, Request, SessionACL,
    TimeOrNow, WriteFlags,
};
use nix::fcntl::{AtFlags, OFlag, openat};
use nix::sys::stat::{Mode, SFlag, fstatat};
use nix::sys::statvfs;
use nix::unistd::{UnlinkatFlags, unlinkat};

#[path = "shared/stderr.rs"]
pub mod stderr;

#[derive(Debug)]
struct CortexFuse {
    projection: FuseV1Projection,
    paths: Mutex<HashMap<u64, String>>,
    lookup_counts: Mutex<HashMap<u64, u64>>,
    socket_overlays: Mutex<HashSet<String>>,
}

impl CortexFuse {
    fn new(root: PathBuf) -> Result<Self, String> {
        let projection = FuseV1Projection::new(root);
        if let Err(_error) = projection.refresh_provider_model_cache() {}
        let root_node = projection
            .root_node()
            .map_err(|error| format!("invalid source root: {}", error.errno()))?;
        let mut paths = HashMap::new();
        paths.insert(root_node.inode(), root_node.abi_path().to_owned());
        Ok(Self {
            projection,
            paths: Mutex::new(paths),
            lookup_counts: Mutex::new(HashMap::new()),
            socket_overlays: Mutex::new(HashSet::new()),
        })
    }

    fn path_for_inode(&self, inode: INodeNo) -> Result<String, FuseV1Error> {
        self.paths
            .lock()
            .map_err(|_error| FuseV1Error::Io)?
            .get(&inode.0)
            .cloned()
            .ok_or(FuseV1Error::NotFound)
    }

    fn remember(&self, node: &FuseV1Node) -> Result<(), FuseV1Error> {
        self.paths
            .lock()
            .map_err(|_error| FuseV1Error::Io)?
            .insert(node.inode(), node.abi_path().to_owned());
        Ok(())
    }

    fn remember_lookup(&self, node: &FuseV1Node) -> Result<(), FuseV1Error> {
        self.remember(node)?;
        let mut counts = self
            .lookup_counts
            .lock()
            .map_err(|_error| FuseV1Error::Io)?;
        *counts.entry(node.inode()).or_insert(0) += 1;
        drop(counts);
        Ok(())
    }

    fn reply_entry(&self, node: &FuseV1Node, reply: ReplyEntry) {
        if let Err(error) = self.remember_lookup(node) {
            reply.error(errno(error));
            return;
        }
        reply.entry(&TTL, &file_attr(node.inode(), node.attr()), Generation(0));
    }

    fn forget_inode(&self, inode: INodeNo, nlookup: u64) -> Result<(), FuseV1Error> {
        if inode.0 == FUSE_V1_ROOT_INODE {
            return Ok(());
        }
        let mut counts = self
            .lookup_counts
            .lock()
            .map_err(|_error| FuseV1Error::Io)?;
        let remove_path = match counts.get_mut(&inode.0) {
            Some(count) if *count > nlookup => {
                *count -= nlookup;
                false
            }
            Some(_count) => {
                counts.remove(&inode.0);
                true
            }
            None => false,
        };
        drop(counts);
        if remove_path {
            self.paths
                .lock()
                .map_err(|_error| FuseV1Error::Io)?
                .remove(&inode.0);
        }
        Ok(())
    }

    fn forget_path(&self, path: &str) -> Result<(), FuseV1Error> {
        self.paths
            .lock()
            .map_err(|_error| FuseV1Error::Io)?
            .retain(|_inode, known| known != path);
        Ok(())
    }

    fn rename_path(&self, from: &str, to: &str) -> Result<(), FuseV1Error> {
        for known in self
            .paths
            .lock()
            .map_err(|_error| FuseV1Error::Io)?
            .values_mut()
        {
            if known == from {
                to.clone_into(known);
            }
        }
        Ok(())
    }
}

macro_rules! path_for_inode_or_reply {
    ($fuse:expr, $inode:expr, $reply:expr) => {
        match $fuse.path_for_inode($inode) {
            Ok(path) => path,
            Err(error) => {
                $reply.error(errno(error));
                return;
            }
        }
    };
}

macro_rules! create_session_layout_child_or_reply {
    ($fuse:expr, $req:expr, $parent:expr, $name:expr, $reply:expr, $method:ident) => {{
        let Some(name) = $name.to_str() else {
            $reply.error(Errno::EINVAL);
            return;
        };
        let parent_path = path_for_inode_or_reply!($fuse, $parent, $reply);
        let Some(path) = child_path(&parent_path, name) else {
            $reply.error(Errno::EINVAL);
            return;
        };
        if let Err(error) = $fuse.projection.$method(&path, $req.uid(), $req.gid()) {
            $reply.error(if matches!(error, FuseV1Error::NotControlFile) {
                readonly_mutation_errno()
            } else {
                errno(error)
            });
            return;
        }
        path
    }};
}

#[path = "../mount/permissions.rs"]
pub mod permissions;
#[macro_use]
#[path = "../mount/init.rs"]
pub mod init;
#[macro_use]
#[path = "../mount/lifecycle.rs"]
pub mod lifecycle;
#[macro_use]
#[path = "../mount/readonly-mutations.rs"]
pub mod readonly_mutations;
#[macro_use]
#[path = "../mount/readdirplus.rs"]
pub mod readdirplus;
#[macro_use]
#[path = "../mount/socket-alias-methods.rs"]
pub mod socket_alias_methods;
#[path = "../mount/filesystem.rs"]
pub mod filesystem;

pub(crate) fn remove_backing_socket_entry(root: &Path, abi_path: &str) -> io::Result<()> {
    let (parent, file_name) = open_backing_socket_parent(root, abi_path)?;
    let stat = fstatat(&parent, file_name.as_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(io::Error::from)?;
    let file_type = SFlag::from_bits_truncate(stat.st_mode);
    if !file_type.contains(SFlag::S_IFSOCK) && !file_type.contains(SFlag::S_IFLNK) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "backing path is not a socket entry",
        ));
    }
    unlinkat(&parent, file_name.as_str(), UnlinkatFlags::NoRemoveDir).map_err(io::Error::from)
}

pub(crate) fn open_backing_socket_parent(
    root: &Path,
    abi_path: &str,
) -> io::Result<(fs::File, String)> {
    let path = Path::new(abi_path);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid socket path"))?
        .to_owned();
    let mut directory = open_single_backing_socket_dir(root)?;
    if let Some(parent) = path.parent() {
        for component in parent.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::Normal(name) => {
                    let name = name.to_str().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "invalid socket path")
                    })?;
                    let next = openat(
                        &directory,
                        name,
                        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(io::Error::from)?;
                    directory = fs::File::from(next);
                }
                std::path::Component::RootDir
                | std::path::Component::ParentDir
                | std::path::Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsupported socket path component",
                    ));
                }
            }
        }
    }
    Ok((directory, file_name))
}

pub(crate) fn open_single_backing_socket_dir(path: &Path) -> io::Result<fs::File> {
    let fd = openat(
        nix::fcntl::AT_FDCWD,
        path,
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    Ok(fs::File::from(fd))
}

#[path = "../mount/helpers.rs"]
pub mod helpers;

pub(crate) use helpers::*;
pub(crate) use init::*;
pub(crate) use permissions::*;
pub(crate) use stderr::*;

pub(crate) fn main() -> ExitCode {
    helpers::main()
}

#[cfg(test)]
#[expect(unused_qualifications, reason = "tests use qualified paths")]
#[path = "cortexfs-mount/tests.rs"]
mod tests;
