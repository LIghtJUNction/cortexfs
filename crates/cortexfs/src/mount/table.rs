/// Mount file syntax error for the fixed v0 mount table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MountError {
    /// Mount line must have exactly four tab-separated fields.
    WrongFieldCount,
    /// Source and target must be absolute paths without tab or newline.
    InvalidPath,
    /// Mode must be `ro` or `rw`.
    InvalidMode,
    /// Option set must be one of the fixed v0 words.
    InvalidOption,
    /// Options other than `-` must not repeat.
    DuplicateOption,
    /// `bind` and `rbind` are mutually exclusive.
    ConflictingBindOption,
}

/// Fixed mount access mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountMode {
    /// Read-only mount.
    ReadOnly,
    /// Read-write mount.
    ReadWrite,
}

impl MountMode {
    /// Parses `ro` or `rw`.
    pub fn parse(value: &str) -> Result<Self, MountError> {
        match value {
            "ro" => Ok(Self::ReadOnly),
            "rw" => Ok(Self::ReadWrite),
            _ => Err(MountError::InvalidMode),
        }
    }
}

/// Fixed v0 mount options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountOption {
    /// Bind mount one path.
    Bind,
    /// Recursive bind mount.
    RecursiveBind,
    /// Disable set-user-ID and set-group-ID behavior.
    NoSuid,
    /// Do not interpret character or block devices.
    NoDev,
    /// Do not execute files.
    NoExec,
}

impl MountOption {
    /// Parses one fixed v0 mount option.
    pub fn parse(value: &str) -> Result<Self, MountError> {
        match value {
            "bind" => Ok(Self::Bind),
            "rbind" => Ok(Self::RecursiveBind),
            "nosuid" => Ok(Self::NoSuid),
            "nodev" => Ok(Self::NoDev),
            "noexec" => Ok(Self::NoExec),
            _ => Err(MountError::InvalidOption),
        }
    }
}

/// One v0 mount table entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountEntry {
    source: String,
    target: String,
    mode: MountMode,
    options: Vec<MountOption>,
}

impl MountEntry {
    /// Parses `source<TAB>target<TAB>mode<TAB>options`.
    pub fn parse(line: &str) -> Result<Self, MountError> {
        let mut fields = line.split('\t');
        let Some(source) = fields.next() else {
            return Err(MountError::WrongFieldCount);
        };
        let Some(target) = fields.next() else {
            return Err(MountError::WrongFieldCount);
        };
        let Some(mode) = fields.next() else {
            return Err(MountError::WrongFieldCount);
        };
        let Some(options) = fields.next() else {
            return Err(MountError::WrongFieldCount);
        };
        if fields.next().is_some() {
            return Err(MountError::WrongFieldCount);
        }
        if !is_absolute_mount_path(source) || !is_absolute_mount_path(target) {
            return Err(MountError::InvalidPath);
        }

        let mode = MountMode::parse(mode)?;
        let options = parse_mount_options(options)?;
        Ok(Self {
            source: source.to_owned(),
            target: target.to_owned(),
            mode,
            options,
        })
    }

    /// Returns the source path.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the target path.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Returns the mount mode.
    #[must_use]
    pub const fn mode(&self) -> MountMode {
        self.mode
    }

    /// Returns mount options.
    #[must_use]
    pub fn options(&self) -> &[MountOption] {
        &self.options
    }

    /// Returns whether this entry is no more permissive than `parent`.
    ///
    /// v0 requires the same source and target. A child may narrow `rw` to `ro`
    /// and may add safety options, but must not remove parent safety options or
    /// make bind traversal broader.
    #[must_use]
    pub fn is_subset_of(&self, parent: &Self) -> bool {
        self.source == parent.source
            && self.target == parent.target
            && mount_mode_allows(parent.mode, self.mode)
            && mount_options_allow(parent.options(), self.options())
    }
}

/// Parsed v0 mount table.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MountTable {
    entries: Vec<MountEntry>,
}

impl MountTable {
    /// Parses a v0 mount table.
    pub fn parse(content: &str) -> Result<Self, MountError> {
        let mut entries = Vec::new();
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            entries.push(MountEntry::parse(line)?);
        }
        Ok(Self { entries })
    }

    /// Returns parsed mount entries.
    #[must_use]
    pub fn entries(&self) -> &[MountEntry] {
        &self.entries
    }

    /// Returns whether every child mount is visible in `parent` with no
    /// expanded authority.
    #[must_use]
    pub fn is_subset_of(&self, parent: &Self) -> bool {
        self.entries.iter().all(|child| {
            parent
                .entries
                .iter()
                .any(|parent_entry| child.is_subset_of(parent_entry))
        })
    }
}

pub(crate) fn mount_mode_allows(parent: MountMode, child: MountMode) -> bool {
    matches!(
        (parent, child),
        (
            MountMode::ReadWrite,
            MountMode::ReadWrite | MountMode::ReadOnly
        ) | (MountMode::ReadOnly, MountMode::ReadOnly)
    )
}

pub(crate) fn mount_options_allow(parent: &[MountOption], child: &[MountOption]) -> bool {
    safety_options_preserved(parent, child) && bind_rank(child) <= bind_rank(parent)
}

pub(crate) fn safety_options_preserved(parent: &[MountOption], child: &[MountOption]) -> bool {
    [MountOption::NoSuid, MountOption::NoDev, MountOption::NoExec]
        .into_iter()
        .all(|option| !parent.contains(&option) || child.contains(&option))
}

pub(crate) fn bind_rank(options: &[MountOption]) -> u8 {
    if options.contains(&MountOption::RecursiveBind) {
        2
    } else {
        u8::from(options.contains(&MountOption::Bind))
    }
}

pub(crate) fn is_absolute_mount_path(value: &str) -> bool {
    if value.bytes().any(|byte| byte.is_ascii_control()) {
        return false;
    }
    if value
        .split('/')
        .any(|component| matches!(component, "." | ".."))
    {
        return false;
    }
    let path = std::path::Path::new(value);
    path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}

pub(crate) fn parse_mount_options(value: &str) -> Result<Vec<MountOption>, MountError> {
    if value == "-" {
        return Ok(Vec::new());
    }
    let mut options = Vec::new();
    for option in value.split(',') {
        let option = MountOption::parse(option)?;
        if options.contains(&option) {
            return Err(MountError::DuplicateOption);
        }
        if matches!(option, MountOption::Bind) && options.contains(&MountOption::RecursiveBind)
            || matches!(option, MountOption::RecursiveBind) && options.contains(&MountOption::Bind)
        {
            return Err(MountError::ConflictingBindOption);
        }
        options.push(option);
    }
    Ok(options)
}
