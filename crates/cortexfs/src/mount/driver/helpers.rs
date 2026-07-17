use super::*;
use crate::is_model_alias;

pub(crate) const TTL: Duration = Duration::from_secs(1);
pub(crate) const S_IFMT: u32 = 0o170_000;
pub(crate) const S_IFREG: u32 = 0o100_000;
pub(crate) const S_IFSOCK: u32 = 0o140_000;

pub(crate) mod statfs;
pub(crate) use statfs::*;

pub(crate) fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = write_error(&format!("cortexfs-mount: {error}"));
            ExitCode::from(2)
        }
    }
}

pub(crate) fn run(args: Vec<OsString>) -> Result<(), String> {
    let config = MountConfig::parse(args)?;
    let source = crate::pin_storage_source(&config.source)
        .map_err(|error| format!("invalid source root: {error}"))?;
    let fs = CortexFuse::new(source.clone())?;
    let mut options = Config::default();
    options.acl = SessionACL::All;
    options.mount_options = vec![
        MountOption::RW,
        MountOption::FSName("cortexfs".to_owned()),
        MountOption::DefaultPermissions,
    ];
    let session = mount_before_refresh(
        || {
            fuser::spawn_mount2(fs, config.mountpoint, &options)
                .map_err(|error| format!("mount failed: {error}"))
        },
        || {
            FuseProjection::new(source)
                .refresh_provider_model_cache()
                .map_err(|error| format!("refresh failed: {error:?}"))
        },
    )?;
    session
        .join()
        .map_err(|error| format!("mount failed: {error}"))
}

/// Mounts once, runs an optional refresh hook, and returns the mounted session.
fn mount_before_refresh<T>(
    mount: impl FnOnce() -> Result<T, String>,
    refresh: impl FnOnce() -> Result<(), String>,
) -> Result<T, String> {
    let session = mount()?;
    let _ignored = refresh();
    Ok(session)
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

pub(crate) fn usage() -> String {
    "usage: cortexfs-mount [--source CTX_ROOT] MOUNTPOINT".to_owned()
}

fn session_append_create_receipt(path: &str, mode: u32, uid: u32, gid: u32) -> Option<FuseNode> {
    FuseProjection::is_session_append_path(path).then(|| {
        FuseNode::new(
            crate::fuse_inode_for_path(path),
            path.to_owned(),
            FuseAttr::with_owner(path.to_owned(), FuseFileType::Regular, 0, mode, uid, gid),
        )
    })
}

#[cfg(test)]
mod session_append_create_receipt_tests {
    use super::*;

    #[test]
    fn mount_is_available_before_blocked_nonfatal_refresh() -> Result<(), String> {
        let (mounted_tx, mounted_rx) = std::sync::mpsc::sync_channel(1);
        let (refresh_tx, refresh_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            mount_before_refresh(
                || {
                    mounted_tx.send(()).map_err(|error| error.to_string())?;
                    Ok("session")
                },
                || {
                    refresh_tx.send(()).map_err(|error| error.to_string())?;
                    release_rx.recv().map_err(|error| error.to_string())?;
                    Err("refresh unavailable".to_owned())
                },
            )
        });

        mounted_rx.recv().map_err(|error| error.to_string())?;
        refresh_rx.recv().map_err(|error| error.to_string())?;
        assert!(!worker.is_finished());
        release_tx.send(()).map_err(|error| error.to_string())?;
        assert_eq!(
            worker
                .join()
                .map_err(|_panic| "mount ordering worker panicked".to_owned())?,
            Ok("session")
        );
        Ok(())
    }

    #[test]
    fn first_history_marker_has_an_independent_create_receipt() -> Result<(), String> {
        let path = "home/1000/agent/coder/session/one/messages.jsonl";
        let node = session_append_create_receipt(path, 0o640, 1000, 1001)
            .ok_or_else(|| "session append path should have a create receipt".to_owned())?;

        assert_eq!(node.inode(), crate::fuse_inode_for_path(path));
        assert_eq!(node.attr().abi_path(), path);
        assert_eq!(node.attr().file_type(), FuseFileType::Regular);
        assert_eq!(node.attr().size(), 0);
        assert_eq!(node.attr().mode(), 0o640);
        assert_eq!(node.attr().uid(), 1000);
        assert_eq!(node.attr().gid(), 1001);
        Ok(())
    }
}

impl CortexFuse {
    pub(crate) fn projected_getattr(&self, path: &str) -> Result<FuseAttr, FuseError> {
        self.projected_node_for_path(path).map(|node| node.attr)
    }

    pub(crate) fn projected_node_for_path(&self, path: &str) -> Result<FuseNode, FuseError> {
        match self.projection.node_for_path(path) {
            Ok(node) => Ok(node),
            Err(FuseError::NotFound) => self
                .socket_overlay(path)?
                .map(|overlay| socket_node(path, overlay.mode, overlay.uid, overlay.gid))
                .ok_or(FuseError::NotFound),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn created_layout_node(
        &self,
        path: &str,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> Result<FuseNode, FuseError> {
        match self.projected_node_for_path(path) {
            Err(FuseError::NotFound) => {
                session_append_create_receipt(path, mode, uid, gid).ok_or(FuseError::NotFound)
            }
            result => result,
        }
    }

    pub(crate) fn projected_lookup(
        &self,
        parent: &FuseNode,
        name: &str,
    ) -> Result<FuseNode, FuseError> {
        let path = child_path(parent.abi_path(), name).ok_or(FuseError::InvalidPath)?;
        self.projected_node_for_path(&path)
    }

    pub(crate) fn projected_readdir(&self, path: &str) -> Result<Vec<FuseDirEntry>, FuseError> {
        let mut entries = self.projection.readdir(path)?;
        let overlays = self
            .socket_overlays
            .lock()
            .map_err(|_error| FuseError::Io)?;
        for socket in overlays.keys() {
            if let Some(name) = immediate_child_name(path, socket)
                && !entries.iter().any(|entry| entry.name() == name)
            {
                entries.push(FuseDirEntry::new(name.to_owned(), FuseFileType::Socket));
            }
        }
        drop(overlays);
        entries.sort_by(|left, right| left.name().cmp(right.name()));
        Ok(entries)
    }

    pub(crate) fn socket_overlay(&self, path: &str) -> Result<Option<SocketOverlay>, FuseError> {
        self.socket_overlays
            .lock()
            .map_err(|_error| FuseError::Io)
            .map(|sockets| sockets.get(path).copied())
    }

    pub(crate) fn has_socket_overlay(&self, path: &str) -> Result<bool, FuseError> {
        Ok(self.socket_overlay(path)?.is_some())
    }

    pub(crate) fn insert_socket_overlay(
        &self,
        path: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<(), FuseError> {
        self.socket_overlays
            .lock()
            .map_err(|_error| FuseError::Io)?
            .insert(path.to_owned(), SocketOverlay { uid, gid, mode });
        Ok(())
    }

    pub(crate) fn remove_socket_overlay(&self, path: &str, uid: u32) -> Result<bool, FuseError> {
        let mut sockets = self
            .socket_overlays
            .lock()
            .map_err(|_error| FuseError::Io)?;
        let Some(overlay) = sockets.get(path) else {
            return Ok(false);
        };
        if overlay.uid != uid {
            return Err(FuseError::PermissionDenied);
        }
        sockets.remove(path);
        drop(sockets);
        Ok(true)
    }

    pub(crate) fn set_socket_overlay_mode(
        &self,
        path: &str,
        uid: u32,
        mode: u32,
    ) -> Result<(), FuseError> {
        let mut sockets = self
            .socket_overlays
            .lock()
            .map_err(|_error| FuseError::Io)?;
        let overlay = sockets.get_mut(path).ok_or(FuseError::NotControlFile)?;
        if overlay.uid != uid {
            return Err(FuseError::PermissionDenied);
        }
        overlay.mode = mode & 0o7777;
        drop(sockets);
        Ok(())
    }

    pub(crate) fn socket_child_path(
        &self,
        parent: INodeNo,
        name: &OsStr,
    ) -> Result<String, FuseError> {
        let name = name.to_str().ok_or(FuseError::InvalidPath)?;
        let parent_path = self.path_for_inode(parent)?;
        let path = child_path(&parent_path, name).ok_or(FuseError::InvalidPath)?;
        is_projected_socket_path(&path)
            .then_some(path)
            .ok_or(FuseError::InvalidPath)
    }

    pub(crate) fn socket_alias_child_path(
        &self,
        parent: INodeNo,
        name: &OsStr,
    ) -> Result<String, FuseError> {
        let name = name.to_str().ok_or(FuseError::InvalidPath)?;
        let parent_path = self.path_for_inode(parent)?;
        let path = child_path(&parent_path, name).ok_or(FuseError::InvalidPath)?;
        FuseProjection::is_socket_alias_path(&path)
            .then_some(path)
            .ok_or(FuseError::InvalidPath)
    }

    pub(crate) fn model_alias_child_path(
        &self,
        parent: INodeNo,
        name: &OsStr,
    ) -> Result<String, FuseError> {
        let name = name.to_str().ok_or(FuseError::InvalidPath)?;
        let parent_path = self.path_for_inode(parent)?;
        let path = child_path(&parent_path, name).ok_or(FuseError::InvalidPath)?;
        path.strip_prefix("model/")
            .is_some_and(is_model_alias)
            .then_some(path)
            .ok_or(FuseError::InvalidPath)
    }

    pub(crate) fn model_symlink_child_path(
        &self,
        parent: INodeNo,
        name: &OsStr,
    ) -> Result<String, FuseError> {
        let name = name.to_str().ok_or(FuseError::InvalidPath)?;
        let parent_path = self.path_for_inode(parent)?;
        if parent_path != "model" || !is_object_name(name) {
            return Err(FuseError::InvalidPath);
        }
        Ok(format!("model/{name}"))
    }

    pub(crate) fn unlink_model_path(
        &self,
        parent: INodeNo,
        name: &OsStr,
    ) -> Result<bool, FuseError> {
        if let Ok(path) = self.model_alias_child_path(parent, name) {
            self.projection.remove_model_alias(&path)?;
            self.forget_path(&path)?;
            return Ok(true);
        }
        let Ok(path) = self.model_symlink_child_path(parent, name) else {
            return Ok(false);
        };
        if !remove_backing_model_alias_symlink(self.projection.root(), &path)? {
            return Ok(false);
        }
        self.forget_path(&path)?;
        Ok(true)
    }

    pub(crate) fn rename_model_alias_path(
        &self,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
    ) -> Result<(), FuseError> {
        let source = self.model_symlink_child_path(parent, name)?;
        let target = self.model_alias_child_path(newparent, newname)?;
        self.projection
            .rename_model_alias_symlink(&source, &target)?;
        self.rename_path(&source, &target)
    }

    pub(crate) fn rename_owner_path(
        &self,
        from: &str,
        to: &str,
        uid: u32,
        flags: RenameFlags,
    ) -> Result<(), FuseError> {
        if flags.is_empty() {
            self.projection.rename_atomic_temp(from, to, uid)?;
        } else if flags == RenameFlags::RENAME_NOREPLACE {
            match self.projection.rename_socket_alias_claim(from, to, uid) {
                Ok(()) => {}
                Err(FuseError::NotControlFile) => {
                    self.projection
                        .rename_atomic_temp_noreplace(from, to, uid)?;
                }
                Err(error) => return Err(error),
            }
        } else {
            return Err(FuseError::InvalidPath);
        }
        self.rename_path(from, to)
    }

    pub(crate) fn xattrs_for_path(&self, path: &str) -> Result<Vec<CortexXattr>, FuseError> {
        let attr = self.projected_getattr(path)?;
        let backing_path = self.projection.root().join(path);
        let backing_metadata = fs::symlink_metadata(&backing_path).ok();
        let backing_exists = backing_metadata.is_some();
        let backing_is_dir = backing_metadata
            .is_some_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink());
        let overlay = self.socket_overlay(path)?.is_some();
        let virtual_projection =
            self.is_virtual_projection_path(path, backing_exists, backing_is_dir, overlay);
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

    pub(crate) fn is_virtual_projection_path(
        &self,
        path: &str,
        backing_exists: bool,
        backing_is_dir: bool,
        overlay: bool,
    ) -> bool {
        overlay
            || path.strip_prefix("model/").is_some_and(|name| {
                is_model_alias(name) && (!backing_exists || self.projection.readlink(path).is_ok())
            })
            || ((path == "model/debug" || path.starts_with("model/debug/echo.d/"))
                && !backing_is_dir)
            || path == "model/debug/echo"
            || matches!(classify_abi_path(path), "ctx.agent.exec" | "ctx.tool.exec")
            || (!backing_exists && self.projection.getattr(path).is_ok())
    }

    pub(crate) fn node_for_dir_entry(
        &self,
        parent_path: &str,
        entry: &FuseDirEntry,
    ) -> Result<FuseNode, FuseError> {
        let parent = self.projected_node_for_path(parent_path)?;
        self.projected_lookup(&parent, entry.name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CortexXattr {
    pub(crate) name: &'static str,
    pub(crate) value: String,
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
pub(crate) struct FuseDirRow {
    pub(crate) inode: u64,
    pub(crate) kind: FileType,
    pub(crate) name: OsString,
}

impl FuseDirRow {
    pub(crate) fn new(inode: u64, kind: FileType, name: impl Into<OsString>) -> Self {
        Self {
            inode,
            kind,
            name: name.into(),
        }
    }
}

pub(crate) fn file_attr(inode: u64, attr: &FuseAttr) -> FileAttr {
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
        nlink: if attr.file_type() == FuseFileType::Directory {
            2
        } else {
            1
        },
        uid: attr.uid(),
        gid: attr.gid(),
        rdev: 0,
        blksize: 4096,
        flags: 0,
    }
}

pub(crate) fn fuser_file_type(kind: FuseFileType) -> FileType {
    match kind {
        FuseFileType::Directory => FileType::Directory,
        FuseFileType::Regular | FuseFileType::Other => FileType::RegularFile,
        FuseFileType::Symlink => FileType::Symlink,
        FuseFileType::Socket => FileType::Socket,
    }
}

pub(crate) fn errno(error: FuseError) -> Errno {
    match error {
        FuseError::InvalidPath
        | FuseError::NotControlFile
        | FuseError::InvalidOffset
        | FuseError::InvalidContent => Errno::EINVAL,
        FuseError::NotFound => Errno::ENOENT,
        FuseError::NotDirectory => Errno::ENOTDIR,
        FuseError::NotFile => Errno::EISDIR,
        FuseError::NotEmpty => Errno::ENOTEMPTY,
        FuseError::AlreadyExists => Errno::EEXIST,
        FuseError::ReadOnly => Errno::EROFS,
        FuseError::TooLarge => Errno::EMSGSIZE,
        FuseError::PermissionDenied => Errno::EACCES,
        FuseError::Io => Errno::EIO,
    }
}

pub(crate) fn parent_inode(
    path: &str,
    paths: &Mutex<HashMap<u64, String>>,
) -> Result<u64, FuseError> {
    if path.is_empty() {
        return Ok(FUSE_ROOT_INODE);
    }
    let parent = Path::new(path)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or("");
    Ok(paths
        .lock()
        .map_err(|_error| FuseError::Io)?
        .iter()
        .find_map(|(inode, known)| (known == parent).then_some(*inode))
        .unwrap_or(FUSE_ROOT_INODE))
}

pub(crate) fn usize_from_u32(value: u32) -> usize {
    match usize::try_from(value) {
        Ok(value) => value,
        Err(_error) => usize::MAX,
    }
}

pub(crate) fn reply_xattr_bytes(bytes: &[u8], size: u32, reply: ReplyXattr) {
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

pub(crate) fn estimate_tokens_from_bytes(bytes: u64) -> u64 {
    if bytes == 0 { 0 } else { bytes.div_ceil(4) }
}

pub(crate) fn child_path(parent: &str, name: &str) -> Option<String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    if parent.is_empty() {
        Some(name.to_owned())
    } else {
        Some(format!("{parent}/{name}"))
    }
}

pub(crate) fn is_projected_socket_path(path: &str) -> bool {
    matches!(
        classify_abi_path(path),
        "ctx.agent.socket" | "ctx.model.socket"
    )
}

pub(crate) fn socket_node(path: &str, mode: u32, uid: u32, gid: u32) -> FuseNode {
    FuseNode::new(
        socket_inode(path),
        path.to_owned(),
        FuseAttr::with_owner(path.to_owned(), FuseFileType::Socket, 0, mode, uid, gid),
    )
}

pub(crate) fn remove_backing_model_alias_symlink(
    root: &Path,
    abi_path: &str,
) -> Result<bool, FuseError> {
    let (parent, file_name) =
        open_backing_socket_parent(root, abi_path).map_err(|_error| FuseError::Io)?;
    let stat = match fstatat(&parent, file_name.as_str(), AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(nix::errno::Errno::ENOENT) => return Ok(true),
        Err(_error) => return Err(FuseError::Io),
    };
    if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFLNK) {
        return Ok(false);
    }
    match unlinkat(&parent, file_name.as_str(), UnlinkatFlags::NoRemoveDir) {
        Ok(()) | Err(nix::errno::Errno::ENOENT) => Ok(true),
        Err(_error) => Err(FuseError::Io),
    }
}

pub(crate) fn socket_inode(path: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash.max(FUSE_ROOT_INODE + 1)
}

pub(crate) fn immediate_child_name<'a>(parent: &str, child: &'a str) -> Option<&'a str> {
    if parent.is_empty() {
        return child.split_once('/').is_none().then_some(child);
    }
    let rest = child.strip_prefix(parent)?.strip_prefix('/')?;
    rest.split_once('/').is_none().then_some(rest)
}
