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
    FuseV1Projection, classify_abi_path,
};
use fuser::{
    BsdFileFlags, Config, Errno, FileAttr, FileHandle, FileType, Filesystem, Generation, INodeNo,
    LockOwner, MountOption, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyWrite, ReplyXattr, Request, SessionACL, TimeOrNow, WriteFlags,
};

const TTL: Duration = Duration::from_secs(1);
const S_IFMT: u32 = 0o170_000;
const S_IFSOCK: u32 = 0o140_000;

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = write_error(&format!("cortexfs-mount: {error}"));
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), String> {
    let config = MountConfig::parse(args)?;
    let fs = CortexFuse::new(config.source)?;
    let mut options = Config::default();
    options.acl = SessionACL::All;
    options.mount_options = vec![
        MountOption::RW,
        MountOption::FSName("cortexfs".to_owned()),
        MountOption::DefaultPermissions,
    ];
    let session = fuser::spawn_mount2(fs, config.mountpoint, &options)
        .map_err(|error| format!("mount failed: {error}"))?;
    session
        .join()
        .map_err(|error| format!("mount failed: {error}"))
}

fn write_error(line: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(line.as_bytes())
        .and_then(|()| stderr.write_all(b"\n"))
}

#[derive(Debug, Eq, PartialEq)]
struct MountConfig {
    source: PathBuf,
    mountpoint: PathBuf,
}

impl MountConfig {
    fn parse(args: Vec<OsString>) -> Result<Self, String> {
        let mut source =
            env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from("/ctx"), PathBuf::from);
        let mut mountpoint = None;
        let mut values = args.into_iter();

        while let Some(value) = values.next() {
            if value == "--source" || value == "-s" {
                let Some(next) = values.next() else {
                    return Err("--source requires a path".to_owned());
                };
                source = PathBuf::from(next);
                continue;
            }
            if value == "--help" || value == "-h" {
                return Err(usage());
            }
            if mountpoint.is_some() {
                return Err("unexpected extra argument".to_owned());
            }
            mountpoint = Some(PathBuf::from(value));
        }

        let Some(mountpoint) = mountpoint else {
            return Err(usage());
        };
        Ok(Self { source, mountpoint })
    }
}

fn usage() -> String {
    "usage: cortexfs-mount [--source CTX_ROOT] MOUNTPOINT".to_owned()
}

#[derive(Debug)]
struct CortexFuse {
    projection: FuseV1Projection,
    paths: Mutex<HashMap<u64, String>>,
    socket_overlays: Mutex<HashSet<String>>,
}

impl CortexFuse {
    fn new(root: PathBuf) -> Result<Self, String> {
        let projection = FuseV1Projection::new(root);
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
}

impl Filesystem for CortexFuse {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let parent_path = match self.path_for_inode(parent) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
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
        let path = match self.path_for_inode(ino) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        match self.projected_getattr(&path) {
            Ok(attr) => reply.attr(&TTL, &file_attr(ino.0, &attr)),
            Err(error) => reply.error(errno(error)),
        }
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        let path = match self.path_for_inode(ino) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
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
        let path = match self.path_for_inode(ino) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
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
        let path = match self.path_for_inode(ino) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
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
        let path = match self.path_for_inode(ino) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
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
        let path = match self.path_for_inode(ino) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
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
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let parent_path = match self.path_for_inode(parent) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let Some(path) = child_path(&parent_path, name) else {
            reply.error(Errno::EINVAL);
            return;
        };
        if !is_projected_socket_path(&path) {
            reply.error(Errno::EINVAL);
            return;
        }
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
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let parent_path = match self.path_for_inode(parent) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        let Some(path) = child_path(&parent_path, name) else {
            reply.error(Errno::EINVAL);
            return;
        };
        if !is_projected_socket_path(&path) {
            reply.error(Errno::EINVAL);
            return;
        }
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
                if fs::remove_file(self.projection.root().join(&path)).is_err() {
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
        let path = match self.path_for_inode(ino) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
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
        let path = match self.path_for_inode(ino) {
            Ok(path) => path,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
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

impl CortexFuse {
    fn projected_getattr(&self, path: &str) -> Result<FuseV1Attr, FuseV1Error> {
        match self.projection.getattr(path) {
            Ok(attr) => Ok(attr),
            Err(FuseV1Error::NotFound) if self.has_socket_overlay(path)? => {
                Ok(socket_attr(path, 0o666))
            }
            Err(error) => Err(error),
        }
    }

    fn projected_node_for_path(&self, path: &str) -> Result<FuseV1Node, FuseV1Error> {
        match self.projection.node_for_path(path) {
            Ok(node) => Ok(node),
            Err(FuseV1Error::NotFound) if self.has_socket_overlay(path)? => {
                Ok(socket_node(path, 0o666))
            }
            Err(error) => Err(error),
        }
    }

    fn projected_lookup(&self, parent: &FuseV1Node, name: &str) -> Result<FuseV1Node, FuseV1Error> {
        match self.projection.lookup(parent, name) {
            Ok(node) => Ok(node),
            Err(FuseV1Error::NotFound) => {
                let Some(path) = child_path(parent.abi_path(), name) else {
                    return Err(FuseV1Error::InvalidPath);
                };
                if self.has_socket_overlay(&path)? {
                    Ok(socket_node(&path, 0o666))
                } else {
                    Err(FuseV1Error::NotFound)
                }
            }
            Err(error) => Err(error),
        }
    }

    fn projected_readdir(&self, path: &str) -> Result<Vec<FuseV1DirEntry>, FuseV1Error> {
        let mut entries = self.projection.readdir(path)?;
        let overlays = self
            .socket_overlays
            .lock()
            .map_err(|_error| FuseV1Error::Io)?;
        for socket in overlays.iter() {
            if let Some(name) = immediate_child_name(path, socket)
                && !entries.iter().any(|entry| entry.name() == name)
            {
                entries.push(FuseV1DirEntry::new(name.to_owned(), FuseV1FileType::Socket));
            }
        }
        drop(overlays);
        entries.sort_by(|left, right| left.name().cmp(right.name()));
        Ok(entries)
    }

    fn has_socket_overlay(&self, path: &str) -> Result<bool, FuseV1Error> {
        self.socket_overlays
            .lock()
            .map_err(|_error| FuseV1Error::Io)
            .map(|sockets| sockets.contains(path))
    }

    fn xattrs_for_path(&self, path: &str) -> Result<Vec<CortexXattr>, FuseV1Error> {
        let attr = self.projected_getattr(path)?;
        let backing_path = self.projection.root().join(path);
        let backing_exists = fs::symlink_metadata(&backing_path).is_ok();
        let overlay = self.has_socket_overlay(path)?;
        let virtual_projection = self.is_virtual_projection_path(path, backing_exists, overlay);
        let (origin, storage, virtual_value) = if overlay {
            ("overlay", "memory", "true")
        } else if virtual_projection {
            ("virtual", "memory", "true")
        } else {
            ("disk", "disk", "false")
        };
        let byte_len = attr.size();
        let token_estimate = estimate_tokens_from_bytes(byte_len);
        let mut attrs = vec![
            CortexXattr::new("user.cortexfs.abi_path", path),
            CortexXattr::new("user.cortexfs.kind", classify_abi_path(path)),
            CortexXattr::new("user.cortexfs.origin", origin),
            CortexXattr::new("user.cortexfs.storage", storage),
            CortexXattr::new("user.cortexfs.virtual", virtual_value),
            CortexXattr::new("user.cortexfs.bytes", byte_len.to_string()),
            CortexXattr::new("user.cortexfs.token_estimate", token_estimate.to_string()),
            CortexXattr::new(
                "user.cortexfs.input_token_estimate",
                token_estimate.to_string(),
            ),
            CortexXattr::new("user.cortexfs.output_token_estimate", "0"),
            CortexXattr::new("user.cortexfs.cache_bytes", "0"),
            CortexXattr::new("user.cortexfs.cache_entries", "0"),
            CortexXattr::new("user.cortexfs.cache_state", "none"),
            CortexXattr::new("user.cortexfs.tokenizer", "byte-estimate-v1"),
            CortexXattr::new(
                "user.cortexfs.backing_exists",
                if backing_exists { "true" } else { "false" },
            ),
        ];
        if backing_exists {
            attrs.push(CortexXattr::new(
                "user.cortexfs.backing_path",
                backing_path.display().to_string(),
            ));
        }
        Ok(attrs)
    }

    fn is_virtual_projection_path(&self, path: &str, backing_exists: bool, overlay: bool) -> bool {
        if overlay {
            return true;
        }
        if matches!(path, "model/main" | "model/helper") {
            return !backing_exists || self.projection.readlink(path).is_ok();
        }
        if path == "model/debug"
            || path == "model/debug/echo"
            || path.starts_with("model/debug/echo.d/")
        {
            return true;
        }
        if matches!(classify_abi_path(path), "ctx.agent.exec" | "ctx.tool.exec") {
            return true;
        }
        if !backing_exists && self.projection.getattr(path).is_ok() {
            return true;
        }
        false
    }

    fn node_for_dir_entry(
        &self,
        parent_path: &str,
        entry: &FuseV1DirEntry,
    ) -> Result<FuseV1Node, FuseV1Error> {
        let parent = self.projected_node_for_path(parent_path)?;
        self.projected_lookup(&parent, entry.name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CortexXattr {
    name: &'static str,
    value: String,
}

impl CortexXattr {
    fn new(name: &'static str, value: impl Into<String>) -> Self {
        Self {
            name,
            value: value.into(),
        }
    }
}

#[derive(Debug)]
struct FuseDirRow {
    inode: u64,
    kind: FileType,
    name: OsString,
}

impl FuseDirRow {
    fn new(inode: u64, kind: FileType, name: impl Into<OsString>) -> Self {
        Self {
            inode,
            kind,
            name: name.into(),
        }
    }
}

fn file_attr(inode: u64, attr: &FuseV1Attr) -> FileAttr {
    let perm: u16 = u16::try_from(attr.mode() & 0o7777).unwrap_or_default();
    FileAttr {
        ino: INodeNo(inode),
        size: attr.size(),
        blocks: attr.size().div_ceil(512),
        atime: UNIX_EPOCH,
        mtime: UNIX_EPOCH,
        ctime: UNIX_EPOCH,
        crtime: UNIX_EPOCH,
        kind: fuser_file_type(attr.file_type()),
        perm,
        nlink: if attr.file_type() == FuseV1FileType::Directory {
            2
        } else {
            1
        },
        uid: attr.uid(),
        gid: attr.gid(),
        rdev: 0,
        blksize: 512,
        flags: 0,
    }
}

fn fuser_file_type(kind: FuseV1FileType) -> FileType {
    match kind {
        FuseV1FileType::Directory => FileType::Directory,
        FuseV1FileType::Regular | FuseV1FileType::Other => FileType::RegularFile,
        FuseV1FileType::Symlink => FileType::Symlink,
        FuseV1FileType::Socket => FileType::Socket,
    }
}

fn errno(error: FuseV1Error) -> Errno {
    match error {
        FuseV1Error::InvalidPath
        | FuseV1Error::NotControlFile
        | FuseV1Error::InvalidOffset
        | FuseV1Error::InvalidContent => Errno::EINVAL,
        FuseV1Error::NotFound => Errno::ENOENT,
        FuseV1Error::NotDirectory => Errno::ENOTDIR,
        FuseV1Error::NotFile => Errno::EISDIR,
        FuseV1Error::TooLarge => Errno::EMSGSIZE,
        FuseV1Error::Io => Errno::EIO,
    }
}

fn parent_inode(path: &str, paths: &Mutex<HashMap<u64, String>>) -> Result<u64, FuseV1Error> {
    if path.is_empty() {
        return Ok(FUSE_V1_ROOT_INODE);
    }
    let parent = Path::new(path)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or("");
    Ok(paths
        .lock()
        .map_err(|_error| FuseV1Error::Io)?
        .iter()
        .find_map(|(inode, known)| (known == parent).then_some(*inode))
        .unwrap_or(FUSE_V1_ROOT_INODE))
}

fn usize_from_u32(value: u32) -> usize {
    match usize::try_from(value) {
        Ok(value) => value,
        Err(_error) => usize::MAX,
    }
}

fn reply_xattr_bytes(bytes: &[u8], size: u32, reply: ReplyXattr) {
    if size == 0 {
        match u32::try_from(bytes.len()) {
            Ok(len) => reply.size(len),
            Err(_error) => reply.error(Errno::ERANGE),
        }
    } else if bytes.len() <= usize_from_u32(size) {
        reply.data(bytes);
    } else {
        reply.error(Errno::ERANGE);
    }
}

fn estimate_tokens_from_bytes(bytes: u64) -> u64 {
    if bytes == 0 { 0 } else { bytes.div_ceil(4) }
}

fn child_path(parent: &str, name: &str) -> Option<String> {
    if name.is_empty() || name.contains('/') {
        return None;
    }
    if parent.is_empty() {
        Some(name.to_owned())
    } else {
        Some(format!("{parent}/{name}"))
    }
}

fn is_projected_socket_path(path: &str) -> bool {
    matches!(
        classify_abi_path(path),
        "ctx.agent.socket" | "ctx.model.socket"
    )
}

fn socket_attr(path: &str, mode: u32) -> FuseV1Attr {
    FuseV1Attr::with_owner(path.to_owned(), FuseV1FileType::Socket, 0, mode, 0, 0)
}

fn socket_node(path: &str, mode: u32) -> FuseV1Node {
    FuseV1Node::new(socket_inode(path), path.to_owned(), socket_attr(path, mode))
}

fn socket_inode(path: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash.max(FUSE_V1_ROOT_INODE + 1)
}

fn immediate_child_name<'a>(parent: &str, child: &'a str) -> Option<&'a str> {
    if parent.is_empty() {
        return child.split_once('/').is_none().then_some(child);
    }
    let rest = child.strip_prefix(parent)?.strip_prefix('/')?;
    rest.split_once('/').is_none().then_some(rest)
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/cortexfs_mount_tests.rs"
    ));
}
