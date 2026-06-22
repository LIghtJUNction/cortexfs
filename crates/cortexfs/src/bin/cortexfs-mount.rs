use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cortexfs::{
    FUSE_V1_ROOT_INODE, FuseV1Attr, FuseV1DirEntry, FuseV1Error, FuseV1FileType, FuseV1Node,
    FuseV1Projection,
};
use fuser::{
    BsdFileFlags, Config, Errno, FileAttr, FileHandle, FileType, Filesystem, Generation, INodeNo,
    LockOwner, MountOption, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    ReplyWrite, Request, SessionACL, TimeOrNow, WriteFlags,
};

const TTL: Duration = Duration::from_secs(1);

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
        let parent_node = match self.projection.node_for_path(&parent_path) {
            Ok(node) => node,
            Err(error) => {
                reply.error(errno(error));
                return;
            }
        };
        match self.projection.lookup(&parent_node, name) {
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
        match self.projection.getattr(&path) {
            Ok(attr) => reply.attr(&TTL, &file_attr(ino.0, &attr)),
            Err(error) => reply.error(errno(error)),
        }
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
        let entries = match self.projection.readdir(&path) {
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
}

impl CortexFuse {
    fn node_for_dir_entry(
        &self,
        parent_path: &str,
        entry: &FuseV1DirEntry,
    ) -> Result<FuseV1Node, FuseV1Error> {
        let parent = self.projection.node_for_path(parent_path)?;
        self.projection.lookup(&parent, entry.name())
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use cortexfs::{FUSE_V1_ROOT_INODE, FuseV1Attr, FuseV1FileType};
    use fuser::{FileType, INodeNo};

    use super::{file_attr, parent_inode};

    #[test]
    fn file_attr_maps_projection_attributes_to_fuser_attributes() {
        let attr = FuseV1Attr::new(
            "tool/fs.read".to_owned(),
            FuseV1FileType::Regular,
            1025,
            0o644,
        );
        let mapped = file_attr(77, &attr);

        assert_eq!(mapped.ino, INodeNo(77));
        assert_eq!(mapped.size, 1025);
        assert_eq!(mapped.blocks, 3);
        assert_eq!(mapped.kind, FileType::RegularFile);
        assert_eq!(mapped.perm, 0o644);
        assert_eq!(mapped.nlink, 1);
    }

    #[test]
    fn parent_inode_uses_known_parent_or_root() {
        let paths = Mutex::new(HashMap::from([
            (FUSE_V1_ROOT_INODE, String::new()),
            (42, "agent/coder.d".to_owned()),
        ]));

        assert_eq!(parent_inode("agent/coder.d/status", &paths), Ok(42));
        assert_eq!(parent_inode("agent", &paths), Ok(FUSE_V1_ROOT_INODE));
        assert_eq!(parent_inode("", &paths), Ok(FUSE_V1_ROOT_INODE));
    }
}
