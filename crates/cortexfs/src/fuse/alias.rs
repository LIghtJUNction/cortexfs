use crate::*;

impl FuseProjection {
    /// Persists a temporary model symlink used by atomic alias replacement.
    pub fn set_model_alias_symlink(
        &self,
        abi_path: &str,
        target: &Path,
    ) -> Result<FuseNode, FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !is_model_alias_symlink_path(&normalized) {
            return Err(FuseError::InvalidPath);
        }
        let target = normalize_model_alias_target(target).ok_or(FuseError::InvalidPath)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseError::InvalidPath)?;
        ensure_model_alias_parent(parent)?;
        let parent_dir = open_model_alias_parent(parent)?;
        let file_name = model_alias_path_file_name(&path)?;
        nix::unistd::symlinkat(&target, &parent_dir, file_name).map_err(|_error| FuseError::Io)?;
        parent_dir.sync_all().map_err(|_error| FuseError::Io)?;
        model_alias_symlink_node(normalized, &target)
    }

    /// Persists a model alias symlink such as `model/main`.
    pub fn set_model_alias(&self, abi_path: &str, target: &Path) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let Some(alias) = model_alias_name(&normalized) else {
            return Err(FuseError::NotControlFile);
        };
        let target = normalize_model_alias_target(target).ok_or(FuseError::InvalidPath)?;
        let path = self.resolve(&format!("model/{alias}"))?;
        let parent = path.parent().ok_or(FuseError::InvalidPath)?;
        ensure_model_alias_parent(parent)?;
        let parent_dir = open_model_alias_parent(parent)?;
        let temporary = create_unique_model_alias_symlink(&parent_dir, alias, &target)?;
        if let Err(_error) =
            nix::fcntl::renameat(&parent_dir, temporary.as_str(), &parent_dir, alias)
        {
            let _ignored = nix::unistd::unlinkat(
                &parent_dir,
                temporary.as_str(),
                nix::unistd::UnlinkatFlags::NoRemoveDir,
            );
            return Err(FuseError::Io);
        }
        parent_dir.sync_all().map_err(|_error| FuseError::Io)
    }

    /// Removes a persisted model alias override, restoring the built-in target.
    pub fn remove_model_alias(&self, abi_path: &str) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if model_alias_name(&normalized).is_none() {
            return Err(FuseError::NotControlFile);
        }
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseError::InvalidPath)?.to_path_buf();
        ensure_model_alias_parent(&parent)?;
        let parent_dir = open_model_alias_parent(&parent)?;
        let file_name = model_alias_path_file_name(&path)?;
        match nix::unistd::unlinkat(
            &parent_dir,
            file_name,
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        ) {
            Ok(()) => parent_dir.sync_all().map_err(|_error| FuseError::Io),
            Err(nix::errno::Errno::ENOENT) => Ok(()),
            Err(_error) => Err(FuseError::Io),
        }
    }

    /// Renames a temporary model symlink onto a canonical model alias.
    pub fn rename_model_alias_symlink(&self, from: &str, to: &str) -> Result<(), FuseError> {
        let from = normalize_fuse_abi_path(from)?;
        let to = normalize_fuse_abi_path(to)?;
        if !is_model_alias_symlink_path(&from) || model_alias_name(&to).is_none() {
            return Err(FuseError::InvalidPath);
        }
        let source = self.resolve(&from)?;
        let source_parent = source.parent().ok_or(FuseError::InvalidPath)?;
        ensure_model_alias_parent(source_parent)?;
        let source_parent_dir = open_model_alias_parent(source_parent)?;
        let source_name = model_alias_path_file_name(&source)?;
        let target = nix::fcntl::readlinkat(&source_parent_dir, source_name)
            .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
        let target = PathBuf::from(target);
        self.set_model_alias(&to, &target)?;
        if from != to {
            match nix::unistd::unlinkat(
                &source_parent_dir,
                source_name,
                nix::unistd::UnlinkatFlags::NoRemoveDir,
            ) {
                Ok(()) | Err(nix::errno::Errno::ENOENT) => {}
                Err(_error) => return Err(FuseError::Io),
            }
        }
        Ok(())
    }
}

pub(crate) fn ensure_model_alias_parent(parent: &Path) -> Result<(), FuseError> {
    if let Ok(metadata) = fs::symlink_metadata(parent) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_model_alias_parent(parent)
        } else {
            Err(FuseError::Io)
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(parent);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(FuseError::Io);
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(_error) => return Err(FuseError::Io),
        }
    }

    let mut parent_dir =
        if let Some(existing_parent) = missing.last().and_then(|path| path.parent()) {
            open_model_alias_parent(existing_parent)?
        } else {
            return Ok(());
        };

    for directory in missing.iter().rev() {
        let name = model_alias_path_file_name(directory)?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        )
        .map_err(|_error| FuseError::Io)?;
        parent_dir.sync_all().map_err(|_error| FuseError::Io)?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|_error| FuseError::Io)?;
        parent_dir = fs::File::from(child);
        parent_dir.sync_all().map_err(|_error| FuseError::Io)?;
    }
    Ok(())
}

pub(crate) fn sync_model_alias_parent(parent: &Path) -> Result<(), FuseError> {
    open_model_alias_parent(parent)?
        .sync_all()
        .map_err(|_error| FuseError::Io)
}

pub(crate) fn open_model_alias_parent(parent: &Path) -> Result<fs::File, FuseError> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(parent)
        .map_err(|_error| FuseError::Io)?;
    if !directory
        .metadata()
        .map_err(|_error| FuseError::Io)?
        .is_dir()
    {
        return Err(FuseError::Io);
    }
    Ok(directory)
}

pub(crate) fn create_unique_model_alias_symlink(
    parent_dir: &fs::File,
    alias: &str,
    target: &Path,
) -> Result<String, FuseError> {
    for attempt in 0..32_u32 {
        let temporary = format!(
            ".{alias}.tmp-{}-{}-{attempt}",
            std::process::id(),
            monotonic_alias_nonce()
        );
        match nix::unistd::symlinkat(target, parent_dir, temporary.as_str()) {
            Ok(()) => return Ok(temporary),
            Err(nix::errno::Errno::EEXIST) => {}
            Err(_error) => return Err(FuseError::Io),
        }
    }
    Err(FuseError::Io)
}

pub(crate) fn model_alias_path_file_name(path: &Path) -> Result<&str, FuseError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or(FuseError::InvalidPath)
}

pub(crate) fn monotonic_alias_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

pub(crate) fn normalize_model_alias_target(target: &Path) -> Option<PathBuf> {
    let raw = target.to_str()?;
    let model = raw.strip_prefix("/ctx/model/").unwrap_or(raw);
    is_model_name(model).then(|| PathBuf::from(format!("/ctx/model/{model}")))
}

pub(crate) fn is_model_alias_symlink_path(abi_path: &str) -> bool {
    let Some(name) = abi_path.strip_prefix("model/") else {
        return false;
    };
    !name.contains('/') && is_object_name(name)
}

pub(crate) fn model_alias_symlink_node(
    abi_path: String,
    target: &Path,
) -> Result<FuseNode, FuseError> {
    let size = u64::try_from(target.as_os_str().len()).map_err(|_error| FuseError::Io)?;
    Ok(FuseNode::new(
        fuse_inode_for_path(&abi_path),
        abi_path.clone(),
        FuseAttr::with_owner(abi_path, FuseFileType::Symlink, size, 0o777, 0, 0),
    ))
}
