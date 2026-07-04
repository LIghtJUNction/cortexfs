/// Object layout validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectLayoutIssue {
    /// Executable entry is missing.
    MissingExecutable(String),
    /// Executable entry exists but is not executable.
    NotExecutable(String),
    /// Control directory is missing.
    MissingControlDirectory(String),
    /// Control path exists but is not a directory.
    NotControlDirectory(String),
    /// Required control file is missing.
    MissingControlFile(String),
    /// Required control path exists but is not a regular file.
    NotControlFile(String),
    /// Required socket is missing.
    MissingSocket(String),
    /// Socket path exists but is not a Unix socket.
    NotSocket(String),
    /// Control file has a value outside the v1 stable set.
    InvalidControlValue { path: String, value: String },
}

impl ObjectLayoutIssue {
    /// Returns a stable short description of the issue kind.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match *self {
            Self::MissingExecutable(_) => "missing executable",
            Self::NotExecutable(_) => "not executable",
            Self::MissingControlDirectory(_) => "missing control directory",
            Self::NotControlDirectory(_) => "not control directory",
            Self::MissingControlFile(_) => "missing control file",
            Self::NotControlFile(_) => "not control file",
            Self::MissingSocket(_) => "missing socket",
            Self::NotSocket(_) => "not socket",
            Self::InvalidControlValue { .. } => "invalid control value",
        }
    }

    /// Returns the relative ABI path associated with the issue.
    #[must_use]
    pub fn path(&self) -> &str {
        match *self {
            Self::MissingExecutable(ref path)
            | Self::NotExecutable(ref path)
            | Self::MissingControlDirectory(ref path)
            | Self::NotControlDirectory(ref path)
            | Self::MissingControlFile(ref path)
            | Self::NotControlFile(ref path)
            | Self::MissingSocket(ref path)
            | Self::NotSocket(ref path)
            | Self::InvalidControlValue { ref path, .. } => path,
        }
    }

    /// Returns the invalid value, when the issue records one.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        match *self {
            Self::InvalidControlValue { ref value, .. } => Some(value),
            Self::MissingExecutable(_)
            | Self::NotExecutable(_)
            | Self::MissingControlDirectory(_)
            | Self::NotControlDirectory(_)
            | Self::MissingControlFile(_)
            | Self::NotControlFile(_)
            | Self::MissingSocket(_)
            | Self::NotSocket(_) => None,
        }
    }
}

/// Result of inspecting a model, agent, or tool object triple.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectLayoutReport {
    issues: Vec<ObjectLayoutIssue>,
}

/// Result of installing an executable object wrapper and control directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectBootstrap {
    executable: PathBuf,
    control_dir: PathBuf,
}

/// Result of materializing the documented v1 reference tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceTreeBootstrap {
    root: PathBuf,
}

/// Error while installing a v1 executable object wrapper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectBootstrapError {
    /// Object name is not a valid v1 path component.
    InvalidObjectName,
    /// Wrapper target command is empty or contains an unsafe control byte.
    InvalidWrapperTarget,
    /// Override names a file outside the stable control file set.
    InvalidControlFile,
    /// Override value does not satisfy stable syntax for that control file.
    InvalidControlValue,
    /// Object directories could not be created.
    CannotCreate,
    /// Executable or control files could not be written.
    CannotRecord,
    /// Executable permissions could not be set.
    CannotChmod,
}

/// Error while resolving a provider API key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiKeyResolutionError {
    /// Environment variable, service, or account name is invalid.
    InvalidName,
    /// System keychain command failed in an unexpected way.
    KeychainUnavailable,
}

/// Durable session layout creation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableSessionLayoutError {
    /// Session name is not a valid v1 object name.
    InvalidSessionName,
    /// Initial cwd is not an absolute chroot path.
    InvalidCwd,
    /// Optional model name is not a valid v1 object name.
    InvalidModelName,
    /// Temp sessions are process-local and are not durable.
    TempSessionNotDurable,
    /// Required files or directories could not be created.
    CannotCreate,
}

/// Error while materializing the documented v1 reference tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceTreeError {
    /// A root directory, subdirectory, or ordinary file could not be created.
    CannotCreate,
    /// A stable executable object could not be bootstrapped.
    Object(ObjectBootstrapError),
    /// A durable session layout could not be ensured.
    Session(DurableSessionLayoutError),
    /// A parent-owned child result channel could not be recorded.
    Child(ChildContextRecordError),
    /// A documented symlink could not be created or conflicts with an existing path.
    CannotLink,
    /// A documented socket path could not be created or conflicts with an existing path.
    CannotSocket(std::io::ErrorKind),
    /// A deprecated reference-tree placeholder could not be removed.
    CannotRemove,
    /// A deprecated reference-tree alias could not be removed.
    CannotUnlink,
}

impl DurableSessionLayoutError {
    /// Returns a stable errno name for this layout creation failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSessionName | Self::InvalidCwd | Self::InvalidModelName => "EINVAL",
            Self::TempSessionNotDurable => "ENOENT",
            Self::CannotCreate => "EIO",
        }
    }
}

impl ReferenceTreeError {
    /// Returns a stable errno name for this reference-tree bootstrap failure.
    #[must_use]
    pub fn errno(self) -> &'static str {
        match self {
            Self::CannotSocket(std::io::ErrorKind::PermissionDenied) => "EACCES",
            Self::CannotSocket(std::io::ErrorKind::NotFound)
            | Self::Session(DurableSessionLayoutError::TempSessionNotDurable)
            | Self::Child(ChildContextRecordError::MissingParentSession) => "ENOENT",
            Self::CannotSocket(
                std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::AddrInUse,
            ) => "EEXIST",
            Self::CannotCreate
            | Self::Object(
                ObjectBootstrapError::CannotCreate
                | ObjectBootstrapError::CannotRecord
                | ObjectBootstrapError::CannotChmod,
            )
            | Self::Session(DurableSessionLayoutError::CannotCreate)
            | Self::Child(ChildContextRecordError::CannotRecord)
            | Self::CannotLink
            | Self::CannotSocket(_)
            | Self::CannotRemove
            | Self::CannotUnlink => "EIO",
            Self::Object(
                ObjectBootstrapError::InvalidObjectName
                | ObjectBootstrapError::InvalidWrapperTarget
                | ObjectBootstrapError::InvalidControlFile
                | ObjectBootstrapError::InvalidControlValue,
            )
            | Self::Session(
                DurableSessionLayoutError::InvalidSessionName
                | DurableSessionLayoutError::InvalidCwd
                | DurableSessionLayoutError::InvalidModelName,
            )
            | Self::Child(
                ChildContextRecordError::InvalidChildName
                | ChildContextRecordError::InvalidAgentName
                | ChildContextRecordError::InvalidSessionName
                | ChildContextRecordError::InvalidStatus
                | ChildContextRecordError::InvalidText
                | ChildContextRecordError::InvalidRefs,
            ) => "EINVAL",
        }
    }
}
