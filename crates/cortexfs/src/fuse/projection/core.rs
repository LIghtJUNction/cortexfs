use super::*;

use crate::support::plain::{
    create_plain_dir, open_plain_directory, open_plain_file, path_metadata_no_follow,
    plain_file_name, read_symlink_target,
};

#[cfg(test)]
thread_local! {
static AGENT_WINDOW_LOCK_HOOK: std::cell::RefCell<Option<mpsc::Sender<()>>> =
    const { std::cell::RefCell::new(None) };
}

/// Refreshes discovery and catalog caches, failing if either update fails.
fn refresh_provider_caches<D, C>(discovery: D, catalog: C) -> Result<(), FuseError>
where
    D: FnOnce() -> Result<(), FuseError>,
    C: FnOnce() -> Result<(), FuseError>,
{
    let discovery = discovery();
    let catalog = catalog();
    discovery.and(catalog)
}

impl FuseProjection {
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

    /// Refreshes the provider model-list and catalog caches used by this projection.
    pub fn refresh_provider_model_cache(&self) -> Result<(), FuseError> {
        refresh_provider_caches(
            || {
                refresh_provider_model_cache(
                    &self.provider_config_dir,
                    &self.provider_model_cache_dir,
                )
            },
            || provider::catalog::refresh_model_limit_cache(&self.provider_model_cache_dir),
        )
    }

    /// Returns the backing root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Projects `getattr`.
    pub fn getattr(&self, abi_path: &str) -> Result<FuseAttr, FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(attr) = self.virtual_object_attr(&normalized)? {
            return Ok(attr);
        }
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        let mode = projected_metadata_mode(&normalized, &metadata);
        let size = if metadata.is_file() {
            if let Some(stream) = columnar::Stream::from_abi_path(&normalized) {
                let session = path.parent().ok_or(FuseError::InvalidPath)?;
                match columnar::len(session, stream) {
                    Ok(size) => size,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound && metadata.len() == 0 =>
                    {
                        let peer = match stream {
                            columnar::Stream::Messages => "events.jsonl",
                            columnar::Stream::Events => "messages.jsonl",
                        };
                        match fs::symlink_metadata(session.join(peer)) {
                            Err(peer_error)
                                if peer_error.kind() == std::io::ErrorKind::NotFound =>
                            {
                                0
                            }
                            Ok(_metadata) => return Err(fuse_metadata_error(&error)),
                            Err(peer_error) => return Err(fuse_metadata_error(&peer_error)),
                        }
                    }
                    Err(error) => return Err(fuse_metadata_error(&error)),
                }
            } else {
                metadata.len()
            }
        } else {
            metadata.len()
        };
        Ok(FuseAttr::with_owner(
            normalized,
            fuse_file_type(metadata.file_type()),
            size,
            mode,
            metadata.uid(),
            metadata.gid(),
        ))
    }

    /// Returns the projected root node.
    pub fn root_node(&self) -> Result<FuseNode, FuseError> {
        self.node_for_path("")
    }

    /// Returns the projected node for an ABI path.
    pub fn node_for_path(&self, abi_path: &str) -> Result<FuseNode, FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let attr = self.getattr(&normalized)?;
        Ok(FuseNode::new(
            fuse_inode_for_path(&normalized),
            normalized,
            attr,
        ))
    }

    /// Projects parent/name lookup.
    pub fn lookup(&self, parent: &FuseNode, name: &str) -> Result<FuseNode, FuseError> {
        let child = fuse_join_child_path(parent.abi_path(), name)?;
        self.node_for_path(&child)
    }

    /// Projects `getattr` for a known node.
    pub fn getattr_node(&self, node: &FuseNode) -> Result<FuseAttr, FuseError> {
        self.getattr(node.abi_path())
    }

    /// Projects `readdir`.
    pub fn readdir(&self, abi_path: &str) -> Result<Vec<FuseDirEntry>, FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(mut entries) = self.virtual_model_readdir(&normalized)? {
            entries.retain(|entry| !Self::is_generated_hidden_child(&normalized, entry.name()));
            return Ok(entries);
        }
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_dir() {
            return Err(FuseError::NotDirectory);
        }
        let directory = open_plain_directory(&path).map_err(|error| fuse_metadata_error(&error))?;
        let entries = fs::read_dir(support::plain::proc_fd_path(&directory))
            .map_err(|error| fuse_metadata_error(&error))?;
        let mut output = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| fuse_metadata_error(&error))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_error| FuseError::InvalidPath)?;
            if Self::is_generated_hidden_child(&normalized, &name) {
                continue;
            }
            let stat = nix::sys::stat::fstatat(
                &directory,
                name.as_str(),
                nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
            output.push(FuseDirEntry::new(
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
                || Self::is_session_store_path(&path)
        })
    }

    /// Projects `readdir` for a known node.
    pub fn readdir_node(&self, node: &FuseNode) -> Result<Vec<FuseDirEntry>, FuseError> {
        self.readdir(node.abi_path())
    }

    /// Projects a small text `read`.
    pub fn read_to_string(&self, abi_path: &str) -> Result<String, FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(content) = self.virtual_object_content(&normalized)? {
            return Ok(content);
        }
        let path = self.resolve(&normalized)?;
        let metadata =
            path_metadata_no_follow(&path).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_file() {
            return Err(FuseError::NotFile);
        }
        if let Some(stream) = columnar::Stream::from_abi_path(&normalized) {
            let session = path.parent().ok_or(FuseError::InvalidPath)?;
            return columnar::read_text(session, stream, MAX_FUSE_SMALL_READ_BYTES).map_err(
                |error| match error.kind() {
                    std::io::ErrorKind::InvalidData => FuseError::InvalidContent,
                    _ => fuse_metadata_error(&error),
                },
            );
        }
        if metadata.len() > MAX_FUSE_SMALL_READ_BYTES {
            return Err(FuseError::TooLarge);
        }
        let len = usize::try_from(metadata.len()).map_err(|_error| FuseError::TooLarge)?;
        let mut file = open_plain_file(&path).map_err(|error| fuse_metadata_error(&error))?;
        let mut content = vec![0; len];
        file.read_exact(&mut content)
            .map_err(|_error| FuseError::Io)?;
        String::from_utf8(content).map_err(|_error| FuseError::InvalidContent)
    }

    /// Projects an offset `read`.
    pub fn read_at(&self, abi_path: &str, offset: u64, size: usize) -> Result<Vec<u8>, FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(content) = self.virtual_object_content(&normalized)? {
            return read_bytes_at(content.as_bytes(), offset, size);
        }
        let path = self.resolve(&normalized)?;
        let metadata =
            path_metadata_no_follow(&path).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_file() {
            return Err(FuseError::NotFile);
        }
        if let Some(stream) = columnar::Stream::from_abi_path(&normalized) {
            let session = path.parent().ok_or(FuseError::InvalidPath)?;
            return columnar::read_at(session, stream, offset, size)
                .map_err(|error| fuse_metadata_error(&error));
        }
        let mut file = open_plain_file(&path).map_err(|error| fuse_metadata_error(&error))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_error| FuseError::Io)?;
        let mut buffer = vec![0; size];
        let read = file.read(&mut buffer).map_err(|_error| FuseError::Io)?;
        buffer.truncate(read);
        Ok(buffer)
    }

    /// Projects a symlink target.
    pub fn readlink(&self, abi_path: &str) -> Result<PathBuf, FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(alias) = model_alias_name(&normalized) {
            return self.default_model_alias_target(alias);
        }
        let path = self.resolve(&normalized)?;
        read_symlink_target(&path).map_err(|error| fuse_readlink_error(&error))
    }

    /// Removes one empty durable plain directory.
    pub fn remove_empty_plain_dir(&self, abi_path: &str) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !is_removable_durable_dir_path(&normalized) {
            return Err(FuseError::ReadOnly);
        }
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(FuseError::NotDirectory);
        }
        fs::remove_dir(&path).map_err(|error| fuse_remove_dir_error(&error))
    }

    /// Removes one owner-authorized agent lifecycle file without following links.
    pub fn remove_layout_file(&self, abi_path: &str, uid: u32) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let target =
            Self::layout_atomic_temp_target(&normalized).unwrap_or_else(|| normalized.clone());
        if !Self::is_agent_wrapper_path(&target) && Self::agent_control_target(&target).is_none() {
            return Err(FuseError::NotControlFile);
        }
        self.authorize_layout_path(&normalized, uid)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseError::InvalidPath)?;
        let directory = open_plain_directory(parent).map_err(|_error| FuseError::Io)?;
        let name = plain_file_name(&path).map_err(|_error| FuseError::Io)?;
        let stat =
            nix::sys::stat::fstatat(&directory, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
        if !nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFREG)
        {
            return Err(FuseError::InvalidPath);
        }
        if stat.st_uid != uid {
            return Err(FuseError::PermissionDenied);
        }
        nix::unistd::unlinkat(&directory, name, nix::unistd::UnlinkatFlags::NoRemoveDir)
            .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
        directory.sync_all().map_err(|_error| FuseError::Io)
    }

    /// Removes one empty owner-authorized agent lifecycle control directory.
    pub fn remove_empty_layout_dir(&self, abi_path: &str, uid: u32) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !Self::is_agent_lifecycle_dir_path(&normalized) {
            return Err(FuseError::NotControlFile);
        }
        self.authorize_layout_path(&normalized, uid)?;
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(FuseError::NotDirectory);
        }
        if metadata.uid() != uid {
            return Err(FuseError::PermissionDenied);
        }
        support::plain::remove_plain_dir(&path).map_err(|error| fuse_remove_dir_error(&error))
    }

    /// Projects a same-directory atomic write for control files.
    pub fn write_control_file(&self, abi_path: &str, content: &str) -> Result<(), FuseError> {
        self.write_control_file_at(abi_path, 0, content.as_bytes())
    }

    /// Projects an offset write for control files.
    ///
    /// The projection only accepts whole-file, same-directory atomic replacement. A FUSE
    /// adapter should collect one small control-file payload and submit it at
    /// offset zero.
    pub fn write_control_file_at(
        &self,
        abi_path: &str,
        offset: u64,
        content: &[u8],
    ) -> Result<(), FuseError> {
        if content.len() > MAX_FUSE_SMALL_WRITE_BYTES {
            return Err(FuseError::TooLarge);
        }
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if Self::is_session_store_path(&normalized) {
            return Err(FuseError::NotFound);
        }
        if Self::is_session_append_path(&normalized) {
            return self.append_session_history_at(&normalized, offset, content);
        }
        if offset != 0 {
            if Self::is_agent_log_control_path(&normalized) {
                return self.append_plain_file_at_end(&normalized, offset, content);
            }
            return Err(FuseError::InvalidOffset);
        }
        if Self::is_session_replace_path(&normalized) {
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
                std::str::from_utf8(content).map_err(|_error| FuseError::InvalidContent)?;
            return atomic_replace_text(&path, content).map_err(|_error| FuseError::Io);
        }
        if !is_fuse_writable_control_path(&normalized) {
            return Err(FuseError::NotControlFile);
        }
        let path = self.resolve(&normalized)?;
        let content = std::str::from_utf8(content).map_err(|_error| FuseError::InvalidContent)?;
        validate_model_control_write(&normalized, content)?;
        if projected_provider_model_control_file(
            &self.provider_config_dir,
            &self.provider_model_cache_dir,
            &normalized,
        )?
        .is_some()
            && let Some(parent) = path.parent()
        {
            create_plain_dir(parent).map_err(|_error| FuseError::Io)?;
        }
        atomic_replace_text(&path, content).map_err(|_error| FuseError::Io)
    }

    /// Projects a FUSE write, preserving request ownership for session files.
    pub fn write_fuse_file_at_for_owner(
        &self,
        abi_path: &str,
        offset: u64,
        content: &[u8],
        uid: u32,
        gid: u32,
    ) -> Result<(), FuseError> {
        if content.len() > MAX_FUSE_SMALL_WRITE_BYTES {
            return Err(FuseError::TooLarge);
        }
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if Self::is_session_store_path(&normalized) {
            return Err(FuseError::NotFound);
        }
        if Self::is_session_append_path(&normalized) {
            self.authorize_layout_path(&normalized, uid)?;
            return self.append_session_history_at(&normalized, offset, content);
        }
        if offset != 0 {
            if Self::agent_control_target(&normalized).is_some()
                || Self::is_agent_wrapper_path(&normalized)
                || Self::layout_atomic_temp_target(&normalized).is_some_and(|target| {
                    Self::agent_control_target(&target).is_some()
                        || Self::is_agent_wrapper_path(&target)
                })
            {
                self.authorize_layout_path(&normalized, uid)?;
            }
            return self.write_control_file_at(&normalized, offset, content);
        }
        if Self::is_session_replace_path(&normalized) {
            return self.replace_session_plain_file_for_owner(&normalized, content, uid, gid);
        }
        if let Some(target) = Self::layout_atomic_temp_target(&normalized) {
            if Self::is_session_replace_path(&target) || Self::is_session_append_path(&target) {
                return self.replace_session_plain_file_for_owner(&normalized, content, uid, gid);
            }
            self.authorize_layout_path(&normalized, uid)?;
            Self::validate_agent_layout_content(&target, content, uid)?;
            let _lock = self.lock_agent_window_target(&target)?;
            if let Some((agent, control)) = Self::agent_control_target(&target)
                && matches!(control, "model" | "window")
            {
                self.validate_agent_window_pair(agent, control, content)?;
            }
            return self.replace_session_plain_file_for_owner(&normalized, content, uid, gid);
        }
        if Self::is_agent_wrapper_path(&normalized)
            || Self::agent_control_target(&normalized).is_some()
        {
            self.authorize_layout_path(&normalized, uid)?;
            Self::validate_agent_layout_content(&normalized, content, uid)?;
            let _lock = self.lock_agent_window_target(&normalized)?;
            if let Some((agent, control)) = Self::agent_control_target(&normalized)
                && matches!(control, "model" | "window")
            {
                self.validate_agent_window_pair(agent, control, content)?;
            }
            return self.replace_session_plain_file_for_owner(&normalized, content, uid, gid);
        }
        self.write_control_file_at(&normalized, 0, content)?;
        Self::chown_fuse_plain_path(&self.resolve(&normalized)?, uid, gid)
    }

    pub(crate) fn append_plain_file_at_end(
        &self,
        normalized: &str,
        offset: u64,
        content: &[u8],
    ) -> Result<(), FuseError> {
        std::str::from_utf8(content).map_err(|_error| FuseError::InvalidContent)?;
        let path = self.resolve(normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|_error| FuseError::Io)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(FuseError::Io);
        }
        if offset != metadata.len() {
            return Err(FuseError::InvalidOffset);
        }
        let mut file = fs::OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|_error| FuseError::Io)?;
        file.write_all(content).map_err(|_error| FuseError::Io)?;
        file.sync_all().map_err(|_error| FuseError::Io)
    }

    pub(crate) fn append_session_history_at(
        &self,
        normalized: &str,
        offset: u64,
        content: &[u8],
    ) -> Result<(), FuseError> {
        let stream =
            columnar::Stream::from_abi_path(normalized).ok_or(FuseError::NotControlFile)?;
        let path = self.resolve(normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(FuseError::Io);
        }
        let session = path.parent().ok_or(FuseError::InvalidPath)?;
        let parsed = (|| {
            let content =
                std::str::from_utf8(content).map_err(|_error| FuseError::InvalidContent)?;
            if content.is_empty() {
                return Ok(Vec::new());
            }
            let body = content
                .strip_suffix('\n')
                .ok_or(FuseError::InvalidContent)?;
            if body.contains('\r') {
                return Err(FuseError::InvalidContent);
            }
            let lines = body.split('\n').collect::<Vec<_>>();
            if lines.is_empty()
                || lines
                    .iter()
                    .any(|line| serde_json::from_str::<Value>(line).is_err())
            {
                return Err(FuseError::InvalidContent);
            }
            Ok(lines)
        })();
        let projected_len = columnar::len(session, stream).map_err(|_error| FuseError::Io)?;
        if offset != projected_len {
            return Err(FuseError::InvalidOffset);
        }
        let lines = parsed?;
        let history = columnar::HistoryGuard::exclusive(session).map_err(|_error| FuseError::Io)?;
        let locked_len = history.len(stream).map_err(|_error| FuseError::Io)?;
        if offset != locked_len {
            return Err(FuseError::InvalidOffset);
        }
        if lines.is_empty() {
            return Ok(());
        }
        history
            .refresh_claims()
            .and_then(|()| history.append(stream, &lines))
            .and_then(|()| history.refresh_claims())
            .map_err(|_error| FuseError::Io)
    }

    pub(crate) fn replace_session_plain_file(
        &self,
        normalized: &str,
        content: &[u8],
    ) -> Result<(), FuseError> {
        let content = std::str::from_utf8(content).map_err(|_error| FuseError::InvalidContent)?;
        let path = self.resolve(normalized)?;
        if Self::is_session_append_path(normalized) {
            return atomic_create_text_with_mode(&path, content, 0o600).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    FuseError::AlreadyExists
                } else {
                    FuseError::Io
                }
            });
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(FuseError::Io),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_error) => return Err(FuseError::Io),
        }
        atomic_replace_text(&path, content).map_err(|_error| FuseError::Io)
    }

    pub(crate) fn replace_session_plain_file_for_owner(
        &self,
        normalized: &str,
        content: &[u8],
        uid: u32,
        gid: u32,
    ) -> Result<(), FuseError> {
        self.replace_session_plain_file(normalized, content)?;
        Self::chown_fuse_plain_path(&self.resolve(normalized)?, uid, gid)
    }

    pub(crate) fn chown_fuse_plain_path(path: &Path, uid: u32, gid: u32) -> Result<(), FuseError> {
        nix::unistd::fchownat(
            nix::fcntl::AT_FDCWD,
            path,
            Some(nix::unistd::Uid::from_raw(uid)),
            Some(nix::unistd::Gid::from_raw(gid)),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_error| FuseError::Io)
    }

    /// Creates one whitelisted session or agent-lifecycle directory.
    pub fn create_layout_dir(
        &self,
        abi_path: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !Self::is_session_layout_dir_path(&normalized)
            && !Self::is_agent_lifecycle_dir_path(&normalized)
        {
            return Err(FuseError::NotControlFile);
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
    ) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !Self::is_session_replace_path(&normalized)
            && !Self::is_session_append_path(&normalized)
            && Self::layout_atomic_temp_target(&normalized).is_none()
            && !Self::is_agent_wrapper_path(&normalized)
            && Self::agent_control_target(&normalized).is_none()
        {
            return Err(FuseError::NotControlFile);
        }
        self.authorize_layout_path(&normalized, uid)?;
        self.replace_session_plain_file_for_owner(&normalized, b"", uid, gid)?;
        Self::set_plain_mode(&self.resolve(&normalized)?, mode)
    }

    /// Renames one whitelisted same-directory atomic temp file.
    pub fn rename_atomic_temp(&self, from: &str, to: &str, uid: u32) -> Result<(), FuseError> {
        self.rename_atomic_temp_with(from, to, uid, false)
    }

    /// Creates one missing layout target from its same-directory atomic temp.
    #[doc(hidden)]
    pub fn rename_atomic_temp_noreplace(
        &self,
        from: &str,
        to: &str,
        uid: u32,
    ) -> Result<(), FuseError> {
        self.rename_atomic_temp_with(from, to, uid, true)
    }

    fn rename_atomic_temp_with(
        &self,
        from: &str,
        to: &str,
        uid: u32,
        no_replace: bool,
    ) -> Result<(), FuseError> {
        let from = normalize_fuse_abi_path(from)?;
        let to = normalize_fuse_abi_path(to)?;
        if Self::layout_atomic_temp_target(&from).as_deref() != Some(to.as_str()) {
            return Err(FuseError::NotControlFile);
        }
        let no_replace = no_replace || Self::is_session_append_path(&to);
        self.authorize_layout_path(&from, uid)?;
        self.authorize_layout_path(&to, uid)?;
        let from_path = self.resolve(&from)?;
        let to_path = self.resolve(&to)?;
        let parent = from_path.parent().ok_or(FuseError::InvalidPath)?;
        if to_path.parent() != Some(parent) {
            return Err(FuseError::InvalidPath);
        }
        let parent_dir = open_plain_directory(parent).map_err(|_error| FuseError::Io)?;
        let _agent_lock = self.lock_agent_window_target(&to)?;
        let from_name = plain_file_name(&from_path).map_err(|_error| FuseError::Io)?;
        let to_name = plain_file_name(&to_path).map_err(|_error| FuseError::Io)?;
        let stat = nix::sys::stat::fstatat(
            &parent_dir,
            from_name,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_error| FuseError::Io)?;
        if !nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFREG)
        {
            return Err(FuseError::InvalidPath);
        }
        if let Some((agent, control)) = Self::agent_control_target(&to)
            && matches!(control, "model" | "window")
        {
            let content = support::plain::read_small_text_file(
                &from_path,
                u64::try_from(MAX_FUSE_SMALL_WRITE_BYTES).unwrap_or(u64::MAX),
            )
            .map_err(|_error| FuseError::Io)?;
            Self::validate_agent_layout_content(&to, content.as_bytes(), uid)?;
            self.validate_agent_window_pair(agent, control, content.as_bytes())?;
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
            nix::errno::Errno::EEXIST => FuseError::AlreadyExists,
            nix::errno::Errno::ENOENT => FuseError::NotFound,
            _ => FuseError::Io,
        })?;
        parent_dir.sync_all().map_err(|_error| FuseError::Io)
    }

    /// Applies mode bits to one whitelisted layout path.
    pub fn set_layout_mode(&self, abi_path: &str, mode: u32, uid: u32) -> Result<(), FuseError> {
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
            return Err(FuseError::NotControlFile);
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

    fn set_plain_mode(path: &Path, mode: u32) -> Result<(), FuseError> {
        let metadata = path_metadata_no_follow(path).map_err(|_error| FuseError::Io)?;
        let file = if metadata.is_dir() {
            open_plain_directory(path)
        } else if metadata.is_file() {
            open_plain_file(path)
        } else {
            return Err(FuseError::InvalidPath);
        }
        .map_err(|_error| FuseError::Io)?;
        file.set_permissions(fs::Permissions::from_mode(mode & 0o7777))
            .and_then(|()| file.sync_all())
            .map_err(|_error| FuseError::Io)
    }

    fn create_layout_plain_dir(
        path: &Path,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<(), FuseError> {
        let created = support::plain::create_plain_dir_exclusive(path, mode).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                FuseError::AlreadyExists
            } else {
                FuseError::Io
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
        .map_err(|_error| FuseError::Io)
        .and_then(|()| created.sync_all().map_err(|_error| FuseError::Io));
        if result.is_err() {
            drop(created);
            let _ignored = support::plain::remove_plain_dir(path);
        }
        result
    }

    fn authorize_layout_path(&self, normalized: &str, uid: u32) -> Result<(), FuseError> {
        if let Some((home_uid, agent)) = Self::home_agent_path(normalized) {
            if home_uid != uid {
                return Err(FuseError::PermissionDenied);
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
                Err(_error) => Err(FuseError::Io),
            };
        }
        if let Some(agent) = Self::agent_control_tree_name(normalized) {
            return self.authorize_agent_owner(agent, uid);
        }
        if let Some(agent) = Self::agent_wrapper_name(normalized) {
            return self.authorize_agent_owner(agent, uid);
        }
        Err(FuseError::NotControlFile)
    }

    fn validate_agent_layout_content(
        target: &str,
        content: &[u8],
        uid: u32,
    ) -> Result<(), FuseError> {
        let Some((_agent, control)) = Self::agent_control_target(target) else {
            return std::str::from_utf8(content)
                .map(|_content| ())
                .map_err(|_error| FuseError::InvalidContent);
        };
        let content = std::str::from_utf8(content).map_err(|_error| FuseError::InvalidContent)?;
        if matches!(control, "owner" | "uid") && content.trim().parse::<u32>().ok() != Some(uid) {
            return Err(FuseError::PermissionDenied);
        }
        validate_agent_bootstrap_control_content(control, content)
            .map_err(|_error| FuseError::InvalidContent)
    }

    fn validate_agent_window_pair(
        &self,
        agent: &str,
        candidate_control: &str,
        candidate: &[u8],
    ) -> Result<(), FuseError> {
        let candidate =
            std::str::from_utf8(candidate).map_err(|_error| FuseError::InvalidContent)?;
        let control_dir = self.root.join("agent").join(format!("{agent}.d"));
        // During initial agent materialization the peer control does not exist
        // yet; a missing peer takes its creation default instead of failing.
        let read_peer = |file: &str| match support::plain::read_small_text_file(
            &control_dir.join(file),
            MAX_FUSE_SMALL_READ_BYTES,
        ) {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_error) => Err(FuseError::InvalidContent),
        };
        let model_content = if candidate_control == "model" {
            Some(candidate.to_owned())
        } else {
            read_peer("model")?
        };
        let window_content = if candidate_control == "window" {
            candidate.to_owned()
        } else {
            read_peer("window")?.unwrap_or_else(|| "auto\n".to_owned())
        };
        let setting =
            AgentWindowSetting::parse_control(&window_content).ok_or(FuseError::InvalidContent)?;
        let Some(model_content) = model_content else {
            // No model recorded yet: `auto` stays valid for any later model,
            // while an explicit window cannot be validated against a limit.
            return match setting {
                AgentWindowSetting::Auto => Ok(()),
                AgentWindowSetting::Explicit(_) => Err(FuseError::InvalidContent),
            };
        };
        let model = support::control::parse_canonical_control_value(&model_content)
            .filter(|model| abi::path::is_model_reference(model))
            .ok_or(FuseError::InvalidContent)?;
        let model_name = if is_model_alias(model) {
            let target = self.default_model_alias_target(model)?;
            target
                .to_str()
                .and_then(|target| target.strip_prefix("/ctx/model/"))
                .filter(|target| is_model_name(target))
                .ok_or(FuseError::InvalidContent)?
                .to_owned()
        } else {
            model.to_owned()
        };
        let (provider, model) = model_name
            .split_once('/')
            .ok_or(FuseError::InvalidContent)?;
        let limit_path = format!("model/{provider}/{model}.d/limit");
        let limit_content = self
            .virtual_model_content(&limit_path)?
            .ok_or(FuseError::InvalidContent)?;
        let limit =
            ModelContextLimit::parse_control(&limit_content).ok_or(FuseError::InvalidContent)?;
        setting
            .resolve(limit)
            .map(|_effective| ())
            .map_err(|_error| FuseError::InvalidContent)
    }

    fn lock_agent_window_target(
        &self,
        normalized_target: &str,
    ) -> Result<Option<nix::fcntl::Flock<fs::File>>, FuseError> {
        let Some((agent, control)) = Self::agent_control_target(normalized_target) else {
            return Ok(None);
        };
        if !matches!(control, "model" | "window") {
            return Ok(None);
        }
        let control_dir = open_plain_directory(&self.root.join("agent").join(format!("{agent}.d")))
            .map_err(|_error| FuseError::Io)?;
        #[cfg(test)]
        AGENT_WINDOW_LOCK_HOOK.with(|hook| {
            if let Some(sender) = hook.borrow_mut().take() {
                let _ignored = sender.send(());
            }
        });
        nix::fcntl::Flock::lock(control_dir, nix::fcntl::FlockArg::LockExclusive)
            .map(Some)
            .map_err(|(_dir, _error)| FuseError::Io)
    }

    #[cfg(test)]
    pub(crate) fn set_agent_window_lock_hook(sender: mpsc::Sender<()>) {
        AGENT_WINDOW_LOCK_HOOK.with(|hook| {
            hook.replace(Some(sender));
        });
    }

    pub(crate) fn authorize_agent_owner(&self, agent: &str, uid: u32) -> Result<(), FuseError> {
        let control = self.root.join("agent").join(format!("{agent}.d"));
        let metadata = fs::symlink_metadata(&control).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                FuseError::NotFound
            } else {
                FuseError::Io
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(FuseError::InvalidPath);
        }
        let owner = control.join("owner");
        match support::plain::read_small_text_file(&owner, 64) {
            Ok(owner) => owner
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|owner| *owner == uid)
                .map(|_owner| ())
                .ok_or(FuseError::PermissionDenied),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (metadata.uid() == uid)
                .then_some(())
                .ok_or(FuseError::PermissionDenied),
            Err(_error) => Err(FuseError::InvalidContent),
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
        (is_object_name(agent)
            && (AGENT_CONTROL_FILES.contains(&file)
                || AGENT_OPTIONAL_CONTROL_FILES.contains(&file)))
        .then_some((agent, file))
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

    fn is_session_store_path(normalized: &str) -> bool {
        let Some((session, suffix)) = normalized.split_once("/.store") else {
            return false;
        };
        (suffix.is_empty() || suffix.starts_with('/'))
            && parse_abi_path(session).is_session_instance()
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

    pub(crate) fn resolve(&self, abi_path: &str) -> Result<PathBuf, FuseError> {
        if Self::is_session_store_path(abi_path) {
            return Err(FuseError::NotFound);
        }
        resolve_fuse_abi_path(&self.root, abi_path)
    }
}

pub(crate) fn fuse_readlink_error(error: &std::io::Error) -> FuseError {
    match error.kind() {
        std::io::ErrorKind::NotFound => FuseError::NotFound,
        std::io::ErrorKind::PermissionDenied => FuseError::PermissionDenied,
        _ => FuseError::InvalidPath,
    }
}

pub(crate) fn fuse_remove_dir_error(error: &std::io::Error) -> FuseError {
    match error.kind() {
        std::io::ErrorKind::NotFound => FuseError::NotFound,
        std::io::ErrorKind::NotADirectory => FuseError::NotDirectory,
        std::io::ErrorKind::DirectoryNotEmpty => FuseError::NotEmpty,
        std::io::ErrorKind::PermissionDenied => FuseError::PermissionDenied,
        _ => FuseError::Io,
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

#[cfg(test)]
mod provider_cache_refresh_tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn catalog_refresh_runs_when_discovery_fails() {
        let catalog_ran = Cell::new(false);
        let result = refresh_provider_caches(
            || Err(FuseError::Io),
            || {
                catalog_ran.set(true);
                Ok(())
            },
        );

        assert_eq!(result, Err(FuseError::Io));
        assert!(catalog_ran.get());
    }
}
