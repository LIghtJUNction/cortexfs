use std::path::PathBuf;

/// Running mode for the `CortexFS` FUSE projection.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum MountMode {
    /// Restrict the mount to the process owner.
    #[default]
    SingleUser,
    /// Allow multiple local users, subject to Cortex policy checks.
    MultiUser,
}

/// Static options used before a real FUSE session is started.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MountOptions {
    mountpoint: PathBuf,
    mode: MountMode,
    security: MountSecurityOptions,
}

impl MountOptions {
    /// Creates conservative Linux-first mount options for the given mountpoint.
    #[must_use]
    pub fn new(mountpoint: impl Into<PathBuf>) -> Self {
        Self {
            mountpoint: mountpoint.into(),
            mode: MountMode::default(),
            security: MountSecurityOptions::default(),
        }
    }

    /// Creates multi-user mount options while keeping conservative hardening.
    #[must_use]
    pub fn multi_user(mountpoint: impl Into<PathBuf>) -> Self {
        Self {
            mountpoint: mountpoint.into(),
            mode: MountMode::MultiUser,
            security: MountSecurityOptions::multi_user(),
        }
    }

    /// Returns the target mountpoint.
    #[must_use]
    pub fn mountpoint(&self) -> &PathBuf {
        &self.mountpoint
    }

    /// Returns the configured multi-user mode.
    #[must_use]
    pub const fn mode(&self) -> MountMode {
        self.mode
    }

    /// Enables or disables multi-user mode.
    #[must_use]
    pub fn with_mode(mut self, mode: MountMode) -> Self {
        self.mode = mode;
        self.security = match mode {
            MountMode::SingleUser => MountSecurityOptions::new(),
            MountMode::MultiUser => MountSecurityOptions::multi_user(),
        };
        self
    }

    /// Returns security-focused mount options.
    #[must_use]
    pub const fn security(&self) -> MountSecurityOptions {
        self.security
    }

    /// Replaces security-focused mount options.
    #[must_use]
    pub const fn with_security(mut self, security: MountSecurityOptions) -> Self {
        self.security = security;
        self
    }
}

/// Conservative security options for a Linux `FUSE` mount.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct MountSecurityOptions {
    flags: u8,
}

impl MountSecurityOptions {
    const DEFAULT_PERMISSIONS: u8 = 0b0_0001;
    const ALLOW_OTHER: u8 = 0b0_0010;
    const NOEXEC: u8 = 0b0_0100;
    const NODEV: u8 = 0b0_1000;
    const NOSUID: u8 = 0b1_0000;

    /// Returns conservative single-user mount security options.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            flags: Self::DEFAULT_PERMISSIONS | Self::NOEXEC | Self::NODEV | Self::NOSUID,
        }
    }

    /// Returns multi-user mount security options.
    #[must_use]
    pub const fn multi_user() -> Self {
        Self {
            flags: Self::new().flags | Self::ALLOW_OTHER,
        }
    }

    /// Returns whether kernel default permission checks should be requested.
    #[must_use]
    pub const fn default_permissions(self) -> bool {
        self.has_flag(Self::DEFAULT_PERMISSIONS)
    }

    /// Returns whether `allow_other` should be requested.
    #[must_use]
    pub const fn allow_other(self) -> bool {
        self.has_flag(Self::ALLOW_OTHER)
    }

    /// Returns whether executable files should be disabled in the mount.
    #[must_use]
    pub const fn noexec(self) -> bool {
        self.has_flag(Self::NOEXEC)
    }

    /// Returns whether device files should be disabled in the mount.
    #[must_use]
    pub const fn nodev(self) -> bool {
        self.has_flag(Self::NODEV)
    }

    /// Returns whether setuid/setgid bits should be ignored in the mount.
    #[must_use]
    pub const fn nosuid(self) -> bool {
        self.has_flag(Self::NOSUID)
    }

    const fn has_flag(self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

impl Default for MountSecurityOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for the FUSE projection crate.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FuseConfig {
    options: MountOptions,
}

impl FuseConfig {
    /// Creates a configuration from mount options.
    #[must_use]
    pub const fn new(options: MountOptions) -> Self {
        Self { options }
    }

    /// Returns the mount options.
    #[must_use]
    pub const fn options(&self) -> &MountOptions {
        &self.options
    }
}

/// Mounted FUSE projection marker.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FuseProjection;

impl FuseProjection {
    /// Builds a projection handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for FuseProjection {
    fn default() -> Self {
        Self::new()
    }
}

/// Error type returned by mount scaffolding.
#[derive(Debug)]
pub enum MountError {
    /// Tokio runtime creation failed.
    Runtime(std::io::Error),
    /// FUSE session mount or run failed.
    Fuse(std::io::Error),
}

impl std::fmt::Display for MountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Runtime(ref error) => write!(f, "failed to create mount runtime: {error}"),
            Self::Fuse(ref error) => write!(f, "FUSE mount failed: {error}"),
        }
    }
}

impl std::error::Error for MountError {}
