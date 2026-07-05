const TTL: Duration = Duration::from_secs(1);
const S_IFMT: u32 = 0o170_000;
const S_IFREG: u32 = 0o100_000;
const S_IFSOCK: u32 = 0o140_000;

include!("statfs.rs");

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

    fn socket_child_path(&self, parent: INodeNo, name: &OsStr) -> Result<String, FuseV1Error> {
        let name = name.to_str().ok_or(FuseV1Error::InvalidPath)?;
        let parent_path = self.path_for_inode(parent)?;
        let path = child_path(&parent_path, name).ok_or(FuseV1Error::InvalidPath)?;
        is_projected_socket_path(&path)
            .then_some(path)
            .ok_or(FuseV1Error::InvalidPath)
    }

    fn model_alias_child_path(&self, parent: INodeNo, name: &OsStr) -> Result<String, FuseV1Error> {
        let name = name.to_str().ok_or(FuseV1Error::InvalidPath)?;
        let parent_path = self.path_for_inode(parent)?;
        let path = child_path(&parent_path, name).ok_or(FuseV1Error::InvalidPath)?;
        matches!(path.as_str(), "model/main" | "model/helper")
            .then_some(path)
            .ok_or(FuseV1Error::InvalidPath)
    }

    fn model_symlink_child_path(
        &self,
        parent: INodeNo,
        name: &OsStr,
    ) -> Result<String, FuseV1Error> {
        let name = name.to_str().ok_or(FuseV1Error::InvalidPath)?;
        let parent_path = self.path_for_inode(parent)?;
        if parent_path != "model" || !is_object_name(name) {
            return Err(FuseV1Error::InvalidPath);
        }
        Ok(format!("model/{name}"))
    }

    fn unlink_model_path(&self, parent: INodeNo, name: &OsStr) -> Result<bool, FuseV1Error> {
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

    fn rename_model_alias_path(
        &self,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
    ) -> Result<(), FuseV1Error> {
        let source = self.model_symlink_child_path(parent, name)?;
        let target = self.model_alias_child_path(newparent, newname)?;
        self.projection
            .rename_model_alias_symlink(&source, &target)?;
        self.rename_path(&source, &target)
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
        blksize: 4096,
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
        FuseV1Error::PermissionDenied => Errno::EACCES,
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

fn remove_backing_model_alias_symlink(root: &Path, abi_path: &str) -> Result<bool, FuseV1Error> {
    let (parent, file_name) =
        open_backing_socket_parent(root, abi_path).map_err(|_error| FuseV1Error::Io)?;
    let stat = match fstatat(&parent, file_name.as_str(), AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(nix::errno::Errno::ENOENT) => return Ok(true),
        Err(_error) => return Err(FuseV1Error::Io),
    };
    if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFLNK) {
        return Ok(false);
    }
    match unlinkat(&parent, file_name.as_str(), UnlinkatFlags::NoRemoveDir) {
        Ok(()) | Err(nix::errno::Errno::ENOENT) => Ok(true),
        Err(_error) => Err(FuseV1Error::Io),
    }
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
