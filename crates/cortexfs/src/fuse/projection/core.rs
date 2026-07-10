use super::*;

use crate::plain_fs::{
    create_plain_dir as create_fuse_v1_plain_dir,
    open_plain_directory as open_fuse_v1_plain_directory,
    open_plain_file as open_fuse_v1_plain_file,
    path_metadata_no_follow as fuse_v1_plain_path_metadata,
    plain_file_name as fuse_v1_plain_file_name, read_symlink_target as read_fuse_v1_symlink_target,
};

impl FuseV1Projection {
    /// Creates a local projection over a `/ctx`-shaped root.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            provider_config_dir: PathBuf::from(SYSTEM_PROVIDER_CONFIG_DIR),
            provider_model_cache_dir: PathBuf::from(SYSTEM_PROVIDER_MODEL_CACHE_DIR),
        }
    }

    /// Overrides the provider config directory used for projected models.
    #[must_use]
    pub fn with_provider_config_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.provider_config_dir = path.into();
        self
    }

    /// Overrides the provider model cache directory used for projected models.
    #[must_use]
    pub fn with_provider_model_cache_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.provider_model_cache_dir = path.into();
        self
    }

    /// Refreshes the provider model-list cache used by this projection.
    pub fn refresh_provider_model_cache(&self) -> Result<(), FuseV1Error> {
        refresh_provider_model_cache(&self.provider_config_dir, &self.provider_model_cache_dir)
    }

    /// Returns the backing root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Projects `getattr`.
    pub fn getattr(&self, abi_path: &str) -> Result<FuseV1Attr, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(attr) = self.virtual_object_attr(&normalized)? {
            return Ok(attr);
        }
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        let mode = projected_metadata_mode(&normalized, &metadata);
        Ok(FuseV1Attr::with_owner(
            normalized,
            fuse_file_type(metadata.file_type()),
            metadata.len(),
            mode,
            metadata.uid(),
            metadata.gid(),
        ))
    }

    /// Returns the projected root node.
    pub fn root_node(&self) -> Result<FuseV1Node, FuseV1Error> {
        self.node_for_path("")
    }

    /// Returns the projected node for an ABI path.
    pub fn node_for_path(&self, abi_path: &str) -> Result<FuseV1Node, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let attr = self.getattr(&normalized)?;
        Ok(FuseV1Node::new(
            fuse_v1_inode_for_path(&normalized),
            normalized,
            attr,
        ))
    }

    /// Projects parent/name lookup.
    pub fn lookup(&self, parent: &FuseV1Node, name: &str) -> Result<FuseV1Node, FuseV1Error> {
        let child = fuse_join_child_path(parent.abi_path(), name)?;
        self.node_for_path(&child)
    }

    /// Projects `getattr` for a known node.
    pub fn getattr_node(&self, node: &FuseV1Node) -> Result<FuseV1Attr, FuseV1Error> {
        self.getattr(node.abi_path())
    }

    /// Projects `readdir`.
    pub fn readdir(&self, abi_path: &str) -> Result<Vec<FuseV1DirEntry>, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(mut entries) = self.virtual_model_readdir(&normalized)? {
            entries.retain(|entry| !Self::is_generated_hidden_child(&normalized, entry.name()));
            return Ok(entries);
        }
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_dir() {
            return Err(FuseV1Error::NotDirectory);
        }
        let directory =
            open_fuse_v1_plain_directory(&path).map_err(|error| fuse_metadata_error(&error))?;
        let entries = fs::read_dir(plain_fs::proc_fd_path(&directory))
            .map_err(|error| fuse_metadata_error(&error))?;
        let mut output = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| fuse_metadata_error(&error))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_error| FuseV1Error::InvalidPath)?;
            if Self::is_generated_hidden_child(&normalized, &name) {
                continue;
            }
            let stat = nix::sys::stat::fstatat(
                &directory,
                name.as_str(),
                nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
            output.push(FuseV1DirEntry::new(
                name,
                fuse_file_type_from_mode(stat.st_mode),
            ));
        }
        output.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(output)
    }

    fn is_generated_hidden_child(parent: &str, name: &str) -> bool {
        fuse_join_child_path(parent, name).ok().is_some_and(|path| {
            Self::layout_atomic_temp_target(&path).is_some()
                || Self::is_socket_alias_claim_path(&path)
        })
    }

    /// Projects `readdir` for a known node.
    pub fn readdir_node(&self, node: &FuseV1Node) -> Result<Vec<FuseV1DirEntry>, FuseV1Error> {
        self.readdir(node.abi_path())
    }

    /// Projects a small text `read`.
    pub fn read_to_string(&self, abi_path: &str) -> Result<String, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(content) = self.virtual_object_content(&normalized)? {
            return Ok(content);
        }
        let path = self.resolve(&normalized)?;
        let metadata =
            fuse_v1_plain_path_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_file() {
            return Err(FuseV1Error::NotFile);
        }
        if metadata.len() > MAX_FUSE_V1_SMALL_READ_BYTES {
            return Err(FuseV1Error::TooLarge);
        }
        let len = usize::try_from(metadata.len()).map_err(|_error| FuseV1Error::TooLarge)?;
        let mut file =
            open_fuse_v1_plain_file(&path).map_err(|error| fuse_metadata_error(&error))?;
        let mut content = vec![0; len];
        file.read_exact(&mut content)
            .map_err(|_error| FuseV1Error::Io)?;
        String::from_utf8(content).map_err(|_error| FuseV1Error::InvalidContent)
    }

    /// Projects an offset `read`.
    pub fn read_at(
        &self,
        abi_path: &str,
        offset: u64,
        size: usize,
    ) -> Result<Vec<u8>, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(content) = self.virtual_object_content(&normalized)? {
            return read_bytes_at(content.as_bytes(), offset, size);
        }
        let path = self.resolve(&normalized)?;
        let metadata =
            fuse_v1_plain_path_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_file() {
            return Err(FuseV1Error::NotFile);
        }
        let mut file =
            open_fuse_v1_plain_file(&path).map_err(|error| fuse_metadata_error(&error))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_error| FuseV1Error::Io)?;
        let mut buffer = vec![0; size];
        let read = file.read(&mut buffer).map_err(|_error| FuseV1Error::Io)?;
        buffer.truncate(read);
        Ok(buffer)
    }

    /// Projects a symlink target.
    pub fn readlink(&self, abi_path: &str) -> Result<PathBuf, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(alias) = model_alias_name(&normalized) {
            return self.default_model_alias_target(alias);
        }
        let path = self.resolve(&normalized)?;
        read_fuse_v1_symlink_target(&path).map_err(|error| fuse_readlink_error(&error))
    }

    /// Removes one empty durable plain directory.
    pub fn remove_empty_plain_dir(&self, abi_path: &str) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !is_removable_durable_dir_path(&normalized) {
            return Err(FuseV1Error::ReadOnly);
        }
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(FuseV1Error::NotDirectory);
        }
        fs::remove_dir(&path).map_err(|error| fuse_remove_dir_error(&error))
    }

    /// Removes one owner-authorized agent lifecycle file without following links.
    pub fn remove_layout_file(&self, abi_path: &str, uid: u32) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let target =
            Self::layout_atomic_temp_target(&normalized).unwrap_or_else(|| normalized.clone());
        if !Self::is_agent_wrapper_path(&target) && Self::agent_control_target(&target).is_none() {
            return Err(FuseV1Error::NotControlFile);
        }
        self.authorize_layout_path(&normalized, uid)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseV1Error::InvalidPath)?;
        let directory = open_fuse_v1_plain_directory(parent).map_err(|_error| FuseV1Error::Io)?;
        let name = fuse_v1_plain_file_name(&path).map_err(|_error| FuseV1Error::Io)?;
        let stat =
            nix::sys::stat::fstatat(&directory, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
        if !nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFREG)
        {
            return Err(FuseV1Error::InvalidPath);
        }
        if stat.st_uid != uid {
            return Err(FuseV1Error::PermissionDenied);
        }
        nix::unistd::unlinkat(&directory, name, nix::unistd::UnlinkatFlags::NoRemoveDir)
            .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
        directory.sync_all().map_err(|_error| FuseV1Error::Io)
    }

    /// Removes one empty owner-authorized agent lifecycle control directory.
    pub fn remove_empty_layout_dir(&self, abi_path: &str, uid: u32) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !Self::is_agent_lifecycle_dir_path(&normalized) {
            return Err(FuseV1Error::NotControlFile);
        }
        self.authorize_layout_path(&normalized, uid)?;
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(FuseV1Error::NotDirectory);
        }
        if metadata.uid() != uid {
            return Err(FuseV1Error::PermissionDenied);
        }
        plain_fs::remove_plain_dir(&path).map_err(|error| fuse_remove_dir_error(&error))
    }

    /// Projects a same-directory atomic write for v1 control files.
    pub fn write_control_file(&self, abi_path: &str, content: &str) -> Result<(), FuseV1Error> {
        self.write_control_file_at(abi_path, 0, content.as_bytes())
    }

    /// Projects an offset write for v1 control files.
    ///
    /// v1 only accepts whole-file, same-directory atomic replacement. A FUSE
    /// adapter should collect one small control-file payload and submit it at
    /// offset zero.
    pub fn write_control_file_at(
        &self,
        abi_path: &str,
        offset: u64,
        content: &[u8],
    ) -> Result<(), FuseV1Error> {
        if content.len() > MAX_FUSE_V1_SMALL_WRITE_BYTES {
            return Err(FuseV1Error::TooLarge);
        }
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if offset != 0 {
            if Self::is_agent_log_control_path(&normalized) {
                return self.append_plain_file_at_end(&normalized, offset, content);
            }
            if Self::is_session_append_path(&normalized) {
                return self.append_plain_file_at_end(&normalized, offset, content);
            }
            return Err(FuseV1Error::InvalidOffset);
        }
        if Self::is_session_replace_path(&normalized) {
            return self.replace_session_plain_file(&normalized, content);
        }
        if Self::is_session_append_path(&normalized) {
            return self.replace_session_plain_file(&normalized, content);
        }
        if let Some(target) = Self::layout_atomic_temp_target(&normalized)
            && (Self::is_session_replace_path(&target) || Self::is_session_append_path(&target))
        {
            return self.replace_session_plain_file(&normalized, content);
        }
        if normalized == format!("model/{MODEL_ROUTE_FILE}") {
            let path = self.resolve(&normalized)?;
            let content =
                std::str::from_utf8(content).map_err(|_error| FuseV1Error::InvalidContent)?;
            return atomic_replace_text(&path, content).map_err(|_error| FuseV1Error::Io);
        }
        if !is_fuse_v1_writable_control_path(&normalized) {
            return Err(FuseV1Error::NotControlFile);
        }
        let path = self.resolve(&normalized)?;
        let content = std::str::from_utf8(content).map_err(|_error| FuseV1Error::InvalidContent)?;
        validate_model_control_write(&normalized, content)?;
        if projected_provider_model_control_file(
            &self.provider_config_dir,
            &self.provider_model_cache_dir,
            &normalized,
        )?
        .is_some()
            && let Some(parent) = path.parent()
        {
            create_fuse_v1_plain_dir(parent).map_err(|_error| FuseV1Error::Io)?;
        }
        atomic_replace_text(&path, content).map_err(|_error| FuseV1Error::Io)
    }

    /// Projects a FUSE write, preserving request ownership for session files.
    pub fn write_fuse_file_at_for_owner(
        &self,
        abi_path: &str,
        offset: u64,
        content: &[u8],
        uid: u32,
        gid: u32,
    ) -> Result<(), FuseV1Error> {
        if content.len() > MAX_FUSE_V1_SMALL_WRITE_BYTES {
            return Err(FuseV1Error::TooLarge);
        }
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if offset != 0 {
            if Self::agent_control_target(&normalized).is_some()
                || Self::is_agent_wrapper_path(&normalized)
                || Self::layout_atomic_temp_target(&normalized).is_some_and(|target| {
                    Self::agent_control_target(&target).is_some()
                        || Self::is_agent_wrapper_path(&target)
                })
                || Self::is_session_append_path(&normalized)
            {
                self.authorize_layout_path(&normalized, uid)?;
            }
            return self.write_control_file_at(&normalized, offset, content);
        }
        if Self::is_session_replace_path(&normalized) || Self::is_session_append_path(&normalized) {
            return self.replace_session_plain_file_for_owner(&normalized, content, uid, gid);
        }
        if let Some(target) = Self::layout_atomic_temp_target(&normalized) {
            if Self::is_session_replace_path(&target) || Self::is_session_append_path(&target) {
                return self.replace_session_plain_file_for_owner(&normalized, content, uid, gid);
            }
            self.authorize_layout_path(&normalized, uid)?;
            Self::validate_agent_layout_content(&target, content, uid)?;
            return self.replace_session_plain_file_for_owner(&normalized, content, uid, gid);
        }
        if Self::is_agent_wrapper_path(&normalized)
            || Self::agent_control_target(&normalized).is_some()
        {
            self.authorize_layout_path(&normalized, uid)?;
            Self::validate_agent_layout_content(&normalized, content, uid)?;
            return self.replace_session_plain_file_for_owner(&normalized, content, uid, gid);
        }
        self.write_control_file_at(&normalized, 0, content)?;
        Self::chown_fuse_v1_plain_path(&self.resolve(&normalized)?, uid, gid)
    }

    pub(crate) fn append_plain_file_at_end(
        &self,
        normalized: &str,
        offset: u64,
        content: &[u8],
    ) -> Result<(), FuseV1Error> {
        std::str::from_utf8(content).map_err(|_error| FuseV1Error::InvalidContent)?;
        let path = self.resolve(normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|_error| FuseV1Error::Io)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(FuseV1Error::Io);
        }
        if offset != metadata.len() {
            return Err(FuseV1Error::InvalidOffset);
        }
        let mut file = fs::OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|_error| FuseV1Error::Io)?;
        file.write_all(content).map_err(|_error| FuseV1Error::Io)?;
        file.sync_all().map_err(|_error| FuseV1Error::Io)
    }

    pub(crate) fn replace_session_plain_file(
        &self,
        normalized: &str,
        content: &[u8],
    ) -> Result<(), FuseV1Error> {
        let content = std::str::from_utf8(content).map_err(|_error| FuseV1Error::InvalidContent)?;
        let path = self.resolve(normalized)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(FuseV1Error::Io),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_error) => return Err(FuseV1Error::Io),
        }
        atomic_replace_text(&path, content).map_err(|_error| FuseV1Error::Io)
    }

    pub(crate) fn replace_session_plain_file_for_owner(
        &self,
        normalized: &str,
        content: &[u8],
        uid: u32,
        gid: u32,
    ) -> Result<(), FuseV1Error> {
        self.replace_session_plain_file(normalized, content)?;
        Self::chown_fuse_v1_plain_path(&self.resolve(normalized)?, uid, gid)
    }

    pub(crate) fn chown_fuse_v1_plain_path(
        path: &Path,
        uid: u32,
        gid: u32,
    ) -> Result<(), FuseV1Error> {
        nix::unistd::fchownat(
            nix::fcntl::AT_FDCWD,
            path,
            Some(nix::unistd::Uid::from_raw(uid)),
            Some(nix::unistd::Gid::from_raw(gid)),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_error| FuseV1Error::Io)
    }

    /// Creates one whitelisted session or agent-lifecycle directory.
    pub fn create_layout_dir(
        &self,
        abi_path: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !Self::is_session_layout_dir_path(&normalized)
            && !Self::is_agent_lifecycle_dir_path(&normalized)
        {
            return Err(FuseV1Error::NotControlFile);
        }
        self.authorize_layout_path(&normalized, uid)?;
        let path = self.resolve(&normalized)?;
        Self::create_layout_plain_dir(&path, uid, gid, mode)
    }

    /// Creates one initially empty whitelisted layout file.
    pub fn create_layout_file(
        &self,
        abi_path: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !Self::is_session_replace_path(&normalized)
            && !Self::is_session_append_path(&normalized)
            && Self::layout_atomic_temp_target(&normalized).is_none()
            && !Self::is_agent_wrapper_path(&normalized)
            && Self::agent_control_target(&normalized).is_none()
        {
            return Err(FuseV1Error::NotControlFile);
        }
        self.authorize_layout_path(&normalized, uid)?;
        self.replace_session_plain_file_for_owner(&normalized, b"", uid, gid)?;
        Self::set_plain_mode(&self.resolve(&normalized)?, mode)
    }

    /// Renames one whitelisted same-directory atomic temp file.
    pub fn rename_atomic_temp(&self, from: &str, to: &str, uid: u32) -> Result<(), FuseV1Error> {
        self.rename_atomic_temp_with(from, to, uid, false)
    }

    /// Creates one missing layout target from its same-directory atomic temp.
    #[doc(hidden)]
    pub fn rename_atomic_temp_noreplace(
        &self,
        from: &str,
        to: &str,
        uid: u32,
    ) -> Result<(), FuseV1Error> {
        self.rename_atomic_temp_with(from, to, uid, true)
    }

    fn rename_atomic_temp_with(
        &self,
        from: &str,
        to: &str,
        uid: u32,
        no_replace: bool,
    ) -> Result<(), FuseV1Error> {
        let from = normalize_fuse_abi_path(from)?;
        let to = normalize_fuse_abi_path(to)?;
        if Self::layout_atomic_temp_target(&from).as_deref() != Some(to.as_str()) {
            return Err(FuseV1Error::NotControlFile);
        }
        self.authorize_layout_path(&from, uid)?;
        self.authorize_layout_path(&to, uid)?;
        let from_path = self.resolve(&from)?;
        let to_path = self.resolve(&to)?;
        let parent = from_path.parent().ok_or(FuseV1Error::InvalidPath)?;
        if to_path.parent() != Some(parent) {
            return Err(FuseV1Error::InvalidPath);
        }
        let parent_dir = open_fuse_v1_plain_directory(parent).map_err(|_error| FuseV1Error::Io)?;
        let from_name = fuse_v1_plain_file_name(&from_path).map_err(|_error| FuseV1Error::Io)?;
        let to_name = fuse_v1_plain_file_name(&to_path).map_err(|_error| FuseV1Error::Io)?;
        let stat = nix::sys::stat::fstatat(
            &parent_dir,
            from_name,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_error| FuseV1Error::Io)?;
        if !nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFREG)
        {
            return Err(FuseV1Error::InvalidPath);
        }
        let renamed = if no_replace {
            nix::fcntl::renameat2(
                &parent_dir,
                from_name,
                &parent_dir,
                to_name,
                nix::fcntl::RenameFlags::RENAME_NOREPLACE,
            )
        } else {
            nix::fcntl::renameat(&parent_dir, from_name, &parent_dir, to_name)
        };
        renamed.map_err(|error| match error {
            nix::errno::Errno::EEXIST => FuseV1Error::AlreadyExists,
            nix::errno::Errno::ENOENT => FuseV1Error::NotFound,
            _ => FuseV1Error::Io,
        })?;
        parent_dir.sync_all().map_err(|_error| FuseV1Error::Io)
    }

    /// Applies mode bits to one whitelisted layout path.
    pub fn set_layout_mode(&self, abi_path: &str, mode: u32, uid: u32) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let path = self.resolve(&normalized)?;
        let writable = Self::is_session_layout_dir_path(&normalized)
            || Self::is_agent_lifecycle_dir_path(&normalized)
            || Self::is_session_replace_path(&normalized)
            || Self::is_session_append_path(&normalized)
            || Self::layout_atomic_temp_target(&normalized).is_some()
            || Self::is_agent_wrapper_path(&normalized)
            || Self::agent_control_target(&normalized).is_some();
        if !writable {
            return Err(FuseV1Error::NotControlFile);
        }
        self.authorize_layout_path(&normalized, uid)?;
        Self::set_plain_mode(&path, mode)
    }

    #[doc(hidden)]
    #[must_use]
    pub fn layout_atomic_temp_target(normalized: &str) -> Option<String> {
        let (parent, file_name) = normalized.rsplit_once('/')?;
        let target_name = generated_sibling_target(file_name, "tmp")?;
        let target = format!("{parent}/{target_name}");
        (Self::is_session_replace_path(&target)
            || Self::is_session_append_path(&target)
            || Self::is_agent_wrapper_path(&target)
            || Self::agent_control_target(&target).is_some())
        .then_some(target)
    }

    fn set_plain_mode(path: &Path, mode: u32) -> Result<(), FuseV1Error> {
        let metadata = fuse_v1_plain_path_metadata(path).map_err(|_error| FuseV1Error::Io)?;
        let file = if metadata.is_dir() {
            open_fuse_v1_plain_directory(path)
        } else if metadata.is_file() {
            open_fuse_v1_plain_file(path)
        } else {
            return Err(FuseV1Error::InvalidPath);
        }
        .map_err(|_error| FuseV1Error::Io)?;
        file.set_permissions(fs::Permissions::from_mode(mode & 0o7777))
            .and_then(|()| file.sync_all())
            .map_err(|_error| FuseV1Error::Io)
    }

    fn create_layout_plain_dir(
        path: &Path,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<(), FuseV1Error> {
        let created = plain_fs::create_plain_dir_exclusive(path, mode).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                FuseV1Error::AlreadyExists
            } else {
                FuseV1Error::Io
            }
        })?;
        let result = nix::unistd::fchown(
            &created,
            Some(nix::unistd::Uid::from_raw(uid)),
            Some(nix::unistd::Gid::from_raw(gid)),
        )
        .and_then(|()| {
            nix::sys::stat::fchmod(
                &created,
                nix::sys::stat::Mode::from_bits_truncate(mode & 0o7777),
            )
        })
        .map_err(|_error| FuseV1Error::Io)
        .and_then(|()| created.sync_all().map_err(|_error| FuseV1Error::Io));
        if result.is_err() {
            drop(created);
            let _ignored = plain_fs::remove_plain_dir(path);
        }
        result
    }

    fn authorize_layout_path(&self, normalized: &str, uid: u32) -> Result<(), FuseV1Error> {
        if let Some((home_uid, agent)) = Self::home_agent_path(normalized) {
            if home_uid != uid {
                return Err(FuseV1Error::PermissionDenied);
            }
            return self.authorize_agent_owner(agent, uid);
        }
        if let Some((agent, _control)) = Self::agent_control_target(normalized) {
            return self.authorize_agent_owner(agent, uid);
        }
        if let Some(target) = Self::layout_atomic_temp_target(normalized) {
            return self.authorize_layout_path(&target, uid);
        }
        if let Some(agent) = Self::agent_control_dir_name(normalized) {
            let path = self.resolve(normalized)?;
            return match fs::symlink_metadata(path) {
                Ok(_metadata) => self.authorize_agent_owner(agent, uid),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_error) => Err(FuseV1Error::Io),
            };
        }
        if let Some(agent) = Self::agent_control_tree_name(normalized) {
            return self.authorize_agent_owner(agent, uid);
        }
        if let Some(agent) = Self::agent_wrapper_name(normalized) {
            return self.authorize_agent_owner(agent, uid);
        }
        Err(FuseV1Error::NotControlFile)
    }

    fn validate_agent_layout_content(
        target: &str,
        content: &[u8],
        uid: u32,
    ) -> Result<(), FuseV1Error> {
        let Some((_agent, control)) = Self::agent_control_target(target) else {
            return std::str::from_utf8(content)
                .map(|_content| ())
                .map_err(|_error| FuseV1Error::InvalidContent);
        };
        let content = std::str::from_utf8(content).map_err(|_error| FuseV1Error::InvalidContent)?;
        if matches!(control, "owner" | "uid") && content.trim().parse::<u32>().ok() != Some(uid) {
            return Err(FuseV1Error::PermissionDenied);
        }
        validate_agent_bootstrap_control_content(control, content)
            .map_err(|_error| FuseV1Error::InvalidContent)
    }

    pub(crate) fn authorize_agent_owner(&self, agent: &str, uid: u32) -> Result<(), FuseV1Error> {
        let control = self.root.join("agent").join(format!("{agent}.d"));
        let metadata = fs::symlink_metadata(&control).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                FuseV1Error::NotFound
            } else {
                FuseV1Error::Io
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(FuseV1Error::InvalidPath);
        }
        let owner = control.join("owner");
        match plain_fs::read_small_text_file(&owner, 64) {
            Ok(owner) => owner
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|owner| *owner == uid)
                .map(|_owner| ())
                .ok_or(FuseV1Error::PermissionDenied),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (metadata.uid() == uid)
                .then_some(())
                .ok_or(FuseV1Error::PermissionDenied),
            Err(_error) => Err(FuseV1Error::InvalidContent),
        }
    }

    fn agent_control_dir_name(normalized: &str) -> Option<&str> {
        let mut parts = normalized.split('/');
        let (Some("agent"), Some(control), None) = (parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        let agent = control.strip_suffix(".d")?;
        is_object_name(agent).then_some(agent)
    }

    fn agent_control_tree_name(normalized: &str) -> Option<&str> {
        let mut parts = normalized.split('/');
        let (Some("agent"), Some(control), Some("hooks")) =
            (parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        if !matches!(parts.next(), None | Some("pre.d" | "post.d")) || parts.next().is_some() {
            return None;
        }
        let agent = control.strip_suffix(".d")?;
        is_object_name(agent).then_some(agent)
    }

    fn agent_wrapper_name(normalized: &str) -> Option<&str> {
        let mut parts = normalized.split('/');
        let (Some("agent"), Some(agent), None) = (parts.next(), parts.next(), parts.next()) else {
            return None;
        };
        is_object_name(agent).then_some(agent)
    }

    #[doc(hidden)]
    #[must_use]
    pub fn is_agent_wrapper_path(normalized: &str) -> bool {
        Self::agent_wrapper_name(normalized).is_some()
    }

    fn agent_control_target(normalized: &str) -> Option<(&str, &str)> {
        let mut parts = normalized.split('/');
        let (Some("agent"), Some(control), Some(file), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        let agent = control.strip_suffix(".d")?;
        (is_object_name(agent) && AGENT_CONTROL_FILES.contains(&file)).then_some((agent, file))
    }

    #[doc(hidden)]
    #[must_use]
    pub fn is_agent_control_path(normalized: &str) -> bool {
        Self::agent_control_target(normalized).is_some()
    }

    fn home_agent_path(normalized: &str) -> Option<(u32, &str)> {
        let mut parts = normalized.split('/');
        let (Some("home"), Some(uid), Some("agent"), Some(agent)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        Some((uid.parse().ok()?, is_object_name(agent).then_some(agent)?))
    }

    fn is_agent_log_control_path(normalized: &str) -> bool {
        let mut parts = normalized.split('/');
        let (Some("agent"), Some(control_dir), Some("log"), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        let Some(agent) = control_dir.strip_suffix(".d") else {
            return false;
        };
        is_object_name(agent)
    }

    #[doc(hidden)]
    #[must_use]
    pub fn is_session_append_path(normalized: &str) -> bool {
        let parts = normalized.split('/').collect::<Vec<_>>();
        matches!(
            *parts.as_slice(),
            ["home", uid, "agent", agent, "session", session, file]
                if uid.parse::<u32>().is_ok()
                    && is_object_name(agent)
                    && is_object_name(session)
                    && matches!(file, "messages.jsonl" | "events.jsonl")
        )
    }

    #[doc(hidden)]
    #[must_use]
    pub fn is_session_replace_path(normalized: &str) -> bool {
        let parts = normalized.split('/').collect::<Vec<_>>();
        match *parts.as_slice() {
            ["home", uid, "agent", agent, "session", "index", file]
                if uid.parse::<u32>().is_ok()
                    && is_object_name(agent)
                    && matches!(file, "list" | "current") =>
            {
                true
            }
            [
                "home",
                uid,
                "agent",
                agent,
                "session",
                "index",
                index_kind,
                key,
            ] if uid.parse::<u32>().is_ok()
                && is_object_name(agent)
                && matches!(index_kind, "by-cwd" | "by-hash" | "by-uuid")
                && is_object_name(key) =>
            {
                true
            }
            ["home", uid, "agent", agent, "session", session, file]
                if uid.parse::<u32>().is_ok()
                    && is_object_name(agent)
                    && is_object_name(session)
                    && matches!(
                        file,
                        "latest.md"
                            | "state"
                            | "cwd"
                            | "workspace"
                            | "created_at"
                            | "updated_at"
                            | "meta.json"
                    ) =>
            {
                true
            }
            [
                "home",
                uid,
                "agent",
                agent,
                "session",
                session,
                "context",
                file,
            ] if uid.parse::<u32>().is_ok()
                && is_object_name(agent)
                && is_object_name(session)
                && matches!(
                    file,
                    "budget"
                        | "pack.json"
                        | "pack.md"
                        | "summary.md"
                        | "facts.jsonl"
                        | "decisions.jsonl"
                        | "todo.md"
                        | "refs.jsonl"
                ) =>
            {
                true
            }
            [
                "home",
                uid,
                "agent",
                agent,
                "session",
                session,
                "context",
                cache,
                "index.jsonl",
            ] if uid.parse::<u32>().is_ok()
                && is_object_name(agent)
                && is_object_name(session)
                && matches!(cache, "swap" | "dedup") =>
            {
                true
            }
            _ => false,
        }
    }

    fn is_session_layout_dir_path(normalized: &str) -> bool {
        let parts = normalized.split('/').collect::<Vec<_>>();
        match *parts.as_slice() {
            ["home", uid, "agent", agent, "session"]
                if uid.parse::<u32>().is_ok() && is_object_name(agent) =>
            {
                true
            }
            [
                "home",
                uid,
                "agent",
                agent,
                "session",
                "index",
                ref tail @ ..,
            ] if uid.parse::<u32>().is_ok() && is_object_name(agent) => {
                matches!(tail, [] | ["by-cwd" | "by-hash" | "by-uuid"])
            }
            [
                "home",
                uid,
                "agent",
                agent,
                "session",
                session,
                ref tail @ ..,
            ] if uid.parse::<u32>().is_ok() && is_object_name(agent) && is_object_name(session) => {
                matches!(
                    tail,
                    [] | ["context"]
                        | ["context", "pinned" | "swap" | "dedup" | "child"]
                        | ["context", "swap", "chunk"]
                        | ["context", "dedup", "blob"]
                )
            }
            _ => false,
        }
    }

    fn is_agent_lifecycle_dir_path(normalized: &str) -> bool {
        if Self::agent_control_dir_name(normalized).is_some()
            || Self::agent_control_tree_name(normalized).is_some()
        {
            return true;
        }
        let parts = normalized.split('/').collect::<Vec<_>>();
        match *parts.as_slice() {
            ["home", uid, "agent", agent]
            | [
                "home",
                uid,
                "agent",
                agent,
                "root" | "data" | "cache" | "log" | "session",
            ] if uid.parse::<u32>().is_ok() && is_object_name(agent) => true,
            ["home", uid, "agent", agent, "session", "index"]
            | [
                "home",
                uid,
                "agent",
                agent,
                "session",
                "index",
                "by-cwd" | "by-hash" | "by-uuid",
            ] if uid.parse::<u32>().is_ok() && is_object_name(agent) => true,
            ["home", uid, "agent", agent, "session", session, "terminal"]
                if uid.parse::<u32>().is_ok()
                    && is_object_name(agent)
                    && is_object_name(session) =>
            {
                true
            }
            _ => false,
        }
    }

    pub(crate) fn resolve(&self, abi_path: &str) -> Result<PathBuf, FuseV1Error> {
        resolve_fuse_abi_path(&self.root, abi_path)
    }
}

pub(crate) fn fuse_readlink_error(error: &std::io::Error) -> FuseV1Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => FuseV1Error::NotFound,
        std::io::ErrorKind::PermissionDenied => FuseV1Error::PermissionDenied,
        _ => FuseV1Error::InvalidPath,
    }
}

pub(crate) fn fuse_remove_dir_error(error: &std::io::Error) -> FuseV1Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => FuseV1Error::NotFound,
        std::io::ErrorKind::NotADirectory => FuseV1Error::NotDirectory,
        std::io::ErrorKind::DirectoryNotEmpty => FuseV1Error::NotEmpty,
        std::io::ErrorKind::PermissionDenied => FuseV1Error::PermissionDenied,
        _ => FuseV1Error::Io,
    }
}

pub(crate) fn is_removable_durable_dir_path(normalized: &str) -> bool {
    let parts = normalized.split('/').collect::<Vec<_>>();
    let Some((root, rest)) = parts.split_first() else {
        return false;
    };
    match *root {
        "home" => {
            let Some((uid, tail)) = rest.split_first() else {
                return false;
            };
            !tail.is_empty()
                && uid.parse::<u32>().is_ok()
                && matches!(
                    parse_abi_path(normalized),
                    AbiPathKind::HomeDir
                        | AbiPathKind::SessionRoot
                        | AbiPathKind::SessionDir { .. }
                        | AbiPathKind::Ordinary
                )
        }
        "shared" => {
            let Some((space, tail)) = rest.split_first() else {
                return false;
            };
            !tail.is_empty()
                && is_object_name(space)
                && matches!(
                    parse_abi_path(normalized),
                    AbiPathKind::SharedDir { .. } | AbiPathKind::Ordinary
                )
        }
        _ => false,
    }
}
