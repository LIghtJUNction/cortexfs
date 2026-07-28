use crate::*;

/// File kind exposed by the FUSE projection layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuseFileType {
    /// Directory entry.
    Directory,
    /// Regular file.
    Regular,
    /// Symbolic link.
    Symlink,
    /// Unix domain socket.
    Socket,
    /// Other filesystem object.
    Other,
}

/// Minimal attributes needed by a FUSE adapter for ABI paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuseAttr {
    pub abi_path: String,
    pub file_type: FuseFileType,
    pub size: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

/// Directory entry returned by the FUSE projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuseDirEntry {
    pub name: String,
    pub file_type: FuseFileType,
}

/// Path/inode pair used by a FUSE adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuseNode {
    pub inode: u64,
    pub abi_path: String,
    pub attr: FuseAttr,
}

/// Error returned by the local FUSE projection helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuseError {
    /// ABI path escaped the `/ctx` root or used invalid syntax.
    InvalidPath,
    /// Path does not exist.
    NotFound,
    /// Operation requires a directory.
    NotDirectory,
    /// Operation requires a readable regular file or symlink target.
    NotFile,
    /// Directory is not empty.
    NotEmpty,
    /// Exclusive creation found an existing path.
    AlreadyExists,
    /// Mutation is outside writable durable ABI state.
    ReadOnly,
    /// Writes through this projection are limited to ABI control files.
    NotControlFile,
    /// Control-file write did not start at offset zero.
    InvalidOffset,
    /// Control-file payload was not valid UTF-8 text.
    InvalidContent,
    /// Write exceeds the small-control-file limit.
    TooLarge,
    /// Underlying filesystem denied access.
    PermissionDenied,
    /// Underlying filesystem operation failed.
    Io,
}

/// Local FUSE projection backend over an existing `/ctx`-shaped tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuseProjection {
    pub root: PathBuf,
    pub provider_config_dir: PathBuf,
    pub provider_model_cache_dir: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProjectedFile {
    pub(crate) attr: FuseAttr,
    pub(crate) content: Option<String>,
}

impl FuseAttr {
    /// Creates a projected file attribute record.
    #[must_use]
    pub const fn new(abi_path: String, file_type: FuseFileType, size: u64, mode: u32) -> Self {
        Self::with_owner(abi_path, file_type, size, mode, 0, 0)
    }

    /// Creates a projected file attribute record with source ownership.
    #[must_use]
    pub const fn with_owner(
        abi_path: String,
        file_type: FuseFileType,
        size: u64,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> Self {
        Self {
            abi_path,
            file_type,
            size,
            mode,
            uid,
            gid,
        }
    }

    /// Returns the ABI path relative to `/ctx`.
    #[must_use]
    pub fn abi_path(&self) -> &str {
        &self.abi_path
    }

    /// Returns the projected file kind.
    #[must_use]
    pub const fn file_type(&self) -> FuseFileType {
        self.file_type
    }

    /// Returns the projected byte size.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// Returns Unix mode bits from the backing object.
    #[must_use]
    pub const fn mode(&self) -> u32 {
        self.mode
    }

    /// Returns the Unix owner uid from the backing object.
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    /// Returns the Unix owner gid from the backing object.
    #[must_use]
    pub const fn gid(&self) -> u32 {
        self.gid
    }
}

impl FuseDirEntry {
    /// Creates a projected directory entry.
    #[must_use]
    pub const fn new(name: String, file_type: FuseFileType) -> Self {
        Self { name, file_type }
    }

    /// Returns the entry name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the entry file kind.
    #[must_use]
    pub const fn file_type(&self) -> FuseFileType {
        self.file_type
    }
}

impl FuseNode {
    /// Creates a projected node record.
    #[must_use]
    pub const fn new(inode: u64, abi_path: String, attr: FuseAttr) -> Self {
        Self {
            inode,
            abi_path,
            attr,
        }
    }

    /// Returns the stable inode id.
    #[must_use]
    pub const fn inode(&self) -> u64 {
        self.inode
    }

    /// Returns the ABI path relative to `/ctx`.
    #[must_use]
    pub fn abi_path(&self) -> &str {
        &self.abi_path
    }

    /// Returns projected attributes for this node.
    #[must_use]
    pub const fn attr(&self) -> &FuseAttr {
        &self.attr
    }
}

impl FuseError {
    /// Returns the stable errno name for this projection error.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidPath
            | Self::NotControlFile
            | Self::InvalidOffset
            | Self::InvalidContent => "EINVAL",
            Self::NotFound => "ENOENT",
            Self::NotDirectory => "ENOTDIR",
            Self::NotFile => "EISDIR",
            Self::NotEmpty => "ENOTEMPTY",
            Self::AlreadyExists => "EEXIST",
            Self::ReadOnly => "EROFS",
            Self::TooLarge => "EMSGSIZE",
            Self::PermissionDenied => "EACCES",
            Self::Io => "EIO",
        }
    }
}
