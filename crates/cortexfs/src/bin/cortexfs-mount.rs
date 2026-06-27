#![forbid(unsafe_code)]

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
    FUSE_V1_ROOT_INODE, FuseV1Attr, FuseV1DirEntry, FuseV1Error, FuseV1FileType, FuseV1Node,
    FuseV1Projection, classify_abi_path, is_object_name,
};
use fuser::{
    BsdFileFlags, Config, Errno, FileAttr, FileHandle, FileType, Filesystem, Generation, INodeNo,
    LockOwner, MountOption, OpenFlags, RenameFlags, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyStatfs, ReplyWrite, ReplyXattr, Request, SessionACL, TimeOrNow,
    WriteFlags,
};
use nix::fcntl::{AtFlags, OFlag, openat};
use nix::sys::stat::{Mode, SFlag, fstatat};
use nix::sys::statvfs;
use nix::unistd::{UnlinkatFlags, unlinkat};

#[derive(Debug)]
struct CortexFuse {
    projection: FuseV1Projection,
    paths: Mutex<HashMap<u64, String>>,
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

    fn reply_entry(&self, node: &FuseV1Node, reply: ReplyEntry) {
        if let Err(error) = self.remember(node) {
            reply.error(errno(error));
            return;
        }
        reply.entry(&TTL, &file_attr(node.inode(), node.attr()), Generation(0));
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

impl Filesystem for CortexFuse {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let parent_path = path_for_inode_or_reply!(self, parent, reply);
        let parent_node = match self.projected_node_for_path(&parent_path) {
            Ok(node) => node,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        match self.projected_lookup(&parent_node, name) {
            Ok(node) => self.reply_entry(&node, reply),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        match self.projected_getattr(&path) {
            Ok(attr) => reply.attr(&TTL, &file_attr(ino.0, &attr)),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        let target = match self.projection.readlink(&path) {
            Ok(target) => target,
            Err(_error) => {
                reply.error(Errno::EINVAL);
                return;
            }
        };
        reply.data(target.as_os_str().as_bytes());
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        if mode.is_some() || uid.is_some() || gid.is_some() || flags.is_some() {
            reply.error(Errno::EINVAL);
            return;
        }
        let Some(0) = size else {
            reply.error(Errno::EINVAL);
            return;
        };
        let path = path_for_inode_or_reply!(self, ino, reply);
        if let Err(error) = self.projection.write_control_file_at(&path, 0, b"") {
            reply.error(errno(error));
            return;
        }
        match self.projection.getattr(&path) {
            Ok(attr) => reply.attr(&TTL, &file_attr(ino.0, &attr)),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        let entries = match self.projected_readdir(&path) {
            Ok(entries) => entries,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let mut rows = vec![
            FuseDirRow::new(ino.0, FileType::Directory, "."),
            FuseDirRow::new(
                match parent_inode(&path, &self.paths) {
                    Ok(inode) => inode,
                    Err(error) => {
                        reply.error(errno(error));
                        return;
                    }
                },
                FileType::Directory,
                "..",
            ),
        ];
        for entry in entries {
            match self.node_for_dir_entry(&path, &entry) {
                Ok(node) => {
                    rows.push(FuseDirRow::new(
                        node.inode(),
                        fuser_file_type(entry.file_type()),
                        entry.name(),
                    ));
                    if let Err(error) = self.remember(&node) {
                        reply.error(errno(error));
                        return;
                    }
                }
                Err(error) => {
                    reply.error(errno(error));
                    return;
                }
            }
        }

        let start = match usize::try_from(offset) {
            Ok(start) => start,
            Err(_error) => {
                reply.ok();
                return;
            }
        };
        for (index, row) in rows.into_iter().enumerate().skip(start) {
            let next_offset = u64::try_from(index + 1).unwrap_or(u64::MAX);
            if reply.add(INodeNo(row.inode), next_offset, row.kind, row.name) {
                break;
            }
        }
        reply.ok();
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        let stats = mount_statfs_for_source(self.projection.root());
        reply.statfs(
            stats.blocks,
            stats.blocks_free,
            stats.blocks_available,
            stats.files,
            stats.files_free,
            stats.block_size,
            stats.name_max,
            stats.fragment_size,
        );
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        match self.projection.read_at(&path, offset, usize_from_u32(size)) {
            Ok(bytes) => reply.data(&bytes),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        match self.projection.write_control_file_at(&path, offset, data) {
            Ok(()) => match u32::try_from(data.len()) {
                Ok(count) => reply.written(count),
                Err(_error) => reply.error(Errno::EFBIG),
            },
            Err(error) => reply.error(errno(error)),
        }
    }

    fn mknod(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        umask: u32,
        rdev: u32,
        reply: ReplyEntry,
    ) {
        if rdev != 0 || mode & S_IFMT != S_IFSOCK {
            reply.error(Errno::EINVAL);
            return;
        }
        let path = match self.socket_child_path(parent, name) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        match self.projected_getattr(&path) {
            Ok(attr) if attr.file_type() != FuseV1FileType::Socket => {
                reply.error(Errno::EEXIST);
                return;
            }
            Ok(_attr) => {}
            Err(FuseV1Error::NotFound) => {}
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        }
        if let Err(_error) = self
            .socket_overlays
            .lock()
            .map_err(|_error| FuseV1Error::Io)
            .map(|mut sockets| {
                sockets.insert(path.clone());
            })
        {
            reply.error(Errno::EIO);
            return;
        }
        let permissions = (mode & 0o7777) & !umask;
        self.reply_entry(&socket_node(&path, permissions), reply);
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        match self.unlink_model_path(parent, name) {
            Ok(true) => {
                reply.ok();
                return;
            }
            Ok(false) => {}
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        }
        let path = match self.socket_child_path(parent, name) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let removed_overlay = match self.socket_overlays.lock() {
            Ok(mut sockets) => sockets.remove(&path),
            Err(_error) => {
                reply.error(Errno::EIO);
                return;
            }
        };
        if removed_overlay {
            if let Err(error) = self.forget_path(&path) {
                reply.error(errno(error));
                return;
            }
            reply.ok();
            return;
        }
        match self.projection.getattr(&path) {
            Ok(attr) if attr.file_type() == FuseV1FileType::Socket => {
                if remove_backing_socket_entry(self.projection.root(), &path).is_err() {
                    reply.error(Errno::EIO);
                    return;
                }
                if let Err(error) = self.forget_path(&path) {
                    reply.error(errno(error));
                    return;
                }
                reply.ok();
            }
            Ok(_attr) => reply.error(Errno::EINVAL),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn symlink(
        &self,
        _req: &Request,
        parent: INodeNo,
        link_name: &OsStr,
        target: &Path,
        reply: ReplyEntry,
    ) {
        let path = match self.model_symlink_child_path(parent, link_name) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        match self.projection.set_model_alias_symlink(&path, target) {
            Ok(node) => self.reply_entry(&node, reply),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        if flags.is_empty() {
            match self.rename_model_alias_path(parent, name, newparent, newname) {
                Ok(()) => reply.ok(),
                Err(error) => reply.error(errno(error)),
            }
        } else {
            reply.error(Errno::EINVAL);
        }
    }

    fn setxattr(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _name: &OsStr,
        _value: &[u8],
        _flags: i32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        reply.error(Errno::EROFS);
    }

    fn getxattr(&self, _req: &Request, ino: INodeNo, name: &OsStr, size: u32, reply: ReplyXattr) {
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let path = path_for_inode_or_reply!(self, ino, reply);
        let attrs = match self.xattrs_for_path(&path) {
            Ok(attrs) => attrs,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let Some(value) = attrs
            .iter()
            .find_map(|attr| (attr.name == name).then_some(attr.value.as_bytes()))
        else {
            reply.error(Errno::ENODATA);
            return;
        };
        reply_xattr_bytes(value, size, reply);
    }

    fn listxattr(&self, _req: &Request, ino: INodeNo, size: u32, reply: ReplyXattr) {
        let path = path_for_inode_or_reply!(self, ino, reply);
        let attrs = match self.xattrs_for_path(&path) {
            Ok(attrs) => attrs,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let mut bytes = Vec::new();
        for attr in attrs {
            bytes.extend_from_slice(attr.name.as_bytes());
            bytes.push(0);
        }
        reply_xattr_bytes(&bytes, size, reply);
    }

    fn removexattr(&self, _req: &Request, _ino: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS);
    }
}

fn remove_backing_socket_entry(root: &Path, abi_path: &str) -> io::Result<()> {
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

fn open_backing_socket_parent(root: &Path, abi_path: &str) -> io::Result<(fs::File, String)> {
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

fn open_single_backing_socket_dir(path: &Path) -> io::Result<fs::File> {
    let fd = openat(
        nix::fcntl::AT_FDCWD,
        path,
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    Ok(fs::File::from(fd))
}

include!("../cortexfs_mount_helpers.rs");

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/cortexfs_mount_tests.rs"
    ));
}
