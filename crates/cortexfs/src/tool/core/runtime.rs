use crate::*;

/// Object layout validation uses the shared path-layout issue model.
pub type ObjectLayoutIssue = PathLayoutIssue;

/// Result of inspecting a model, agent, or tool object triple.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectLayoutReport {
    pub(crate) issues: Vec<PathLayoutIssue>,
}

/// Result of installing an executable object wrapper and control directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectBootstrap {
    pub(crate) executable: PathBuf,
    pub(crate) control_dir: PathBuf,
}

/// Result of materializing the documented v1 reference tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceTreeBootstrap {
    pub(crate) root: PathBuf,
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
    /// A newly created entry could not be identity-bound and was retained.
    RetainedResidue,
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
            Self::CannotCreate | Self::RetainedResidue => "EIO",
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
            | Self::Session(
                DurableSessionLayoutError::CannotCreate
                | DurableSessionLayoutError::RetainedResidue,
            )
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
