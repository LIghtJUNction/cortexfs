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
        if let Some(entries) = self.virtual_model_readdir(&normalized)? {
            return Ok(entries);
        }
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_dir() {
            return Err(FuseV1Error::NotDirectory);
        }
        let directory =
            open_fuse_v1_plain_directory(&path).map_err(|error| fuse_metadata_error(&error))?;
        let entries = fs::read_dir(fuse_v1_proc_fd_path(&directory))
            .map_err(|error| fuse_metadata_error(&error))?;
        let mut output = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| fuse_metadata_error(&error))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_error| FuseV1Error::InvalidPath)?;
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
        if Self::session_atomic_temp_target(&normalized).is_some() {
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
            create_fuse_v1_plain_dir(parent)?;
    }
    atomic_replace_text(&path, content).map_err(|_error| FuseV1Error::Io)
}

fn append_plain_file_at_end(
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

    fn replace_session_plain_file(&self, normalized: &str, content: &[u8]) -> Result<(), FuseV1Error> {
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

    /// Creates one directory in the durable session layout.
    ///
    /// This is intentionally narrower than general filesystem `mkdir`: only the
    /// documented session skeleton below `home/<uid>/agent/<agent>/session` is
    /// writable through the v1 FUSE projection.
    pub fn create_session_layout_dir(&self, abi_path: &str) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !Self::is_session_layout_dir_path(&normalized) {
            return Err(FuseV1Error::NotControlFile);
        }
        let path = self.resolve(&normalized)?;
        create_fuse_v1_plain_dir(&path)
    }

    /// Creates an initially empty durable session file.
    pub fn create_session_layout_file(&self, abi_path: &str) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !Self::is_session_replace_path(&normalized)
            && !Self::is_session_append_path(&normalized)
            && Self::session_atomic_temp_target(&normalized).is_none()
        {
            return Err(FuseV1Error::NotControlFile);
        }
        self.replace_session_plain_file(&normalized, b"")
    }

    /// Renames a same-directory durable session atomic temp file to its final file.
    pub fn rename_session_atomic_temp(&self, from: &str, to: &str) -> Result<(), FuseV1Error> {
        let from = normalize_fuse_abi_path(from)?;
        let to = normalize_fuse_abi_path(to)?;
        if Self::session_atomic_temp_target(&from).as_deref() != Some(to.as_str()) {
            return Err(FuseV1Error::NotControlFile);
        }
        let from_path = self.resolve(&from)?;
        let to_path = self.resolve(&to)?;
        let parent = from_path.parent().ok_or(FuseV1Error::InvalidPath)?;
        if to_path.parent() != Some(parent) {
            return Err(FuseV1Error::InvalidPath);
        }
        let parent_dir = open_fuse_v1_plain_directory(parent).map_err(|_error| FuseV1Error::Io)?;
        let from_name = fuse_v1_plain_file_name(&from_path).map_err(|_error| FuseV1Error::Io)?;
        let to_name = fuse_v1_plain_file_name(&to_path).map_err(|_error| FuseV1Error::Io)?;
        nix::fcntl::renameat(&parent_dir, from_name, &parent_dir, to_name)
            .map_err(|_error| FuseV1Error::Io)?;
        parent_dir.sync_all().map_err(|_error| FuseV1Error::Io)
    }

    /// Applies mode bits to a durable session layout path.
    pub fn set_session_layout_mode(&self, abi_path: &str, mode: u32) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let path = self.resolve(&normalized)?;
        if Self::is_session_layout_dir_path(&normalized) {
            let directory = open_fuse_v1_plain_directory(&path).map_err(|_error| FuseV1Error::Io)?;
            directory
                .set_permissions(fs::Permissions::from_mode(mode & 0o7777))
                .and_then(|()| directory.sync_all())
                .map_err(|_error| FuseV1Error::Io)?;
            return Ok(());
        }
        if Self::is_session_replace_path(&normalized) || Self::is_session_append_path(&normalized)
            || Self::session_atomic_temp_target(&normalized).is_some()
        {
            let file = open_fuse_v1_plain_file(&path).map_err(|_error| FuseV1Error::Io)?;
            if !file.metadata().map_err(|_error| FuseV1Error::Io)?.is_file() {
                return Err(FuseV1Error::Io);
            }
            file.set_permissions(fs::Permissions::from_mode(mode & 0o7777))
                .and_then(|()| file.sync_all())
                .map_err(|_error| FuseV1Error::Io)?;
            return Ok(());
        }
        Err(FuseV1Error::NotControlFile)
    }

    fn session_atomic_temp_target(normalized: &str) -> Option<String> {
        let (parent, file_name) = normalized.rsplit_once('/')?;
        let rest = file_name.strip_prefix('.')?;
        let (target_name, _suffix) = rest.split_once(".tmp-")?;
        let target = format!("{parent}/{target_name}");
        (Self::is_session_replace_path(&target) || Self::is_session_append_path(&target))
            .then_some(target)
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

    fn is_session_append_path(normalized: &str) -> bool {
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

    fn is_session_replace_path(normalized: &str) -> bool {
        let parts = normalized.split('/').collect::<Vec<_>>();
        match *parts.as_slice() {
            ["home", uid, "agent", agent, "session", "index", file]
                if uid.parse::<u32>().is_ok()
                    && is_object_name(agent)
                    && matches!(file, "list" | "current") =>
            {
                true
            }
            ["home", uid, "agent", agent, "session", "index", index_kind, key]
                if uid.parse::<u32>().is_ok()
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
                        "latest.md" | "state" | "cwd" | "created_at" | "updated_at" | "meta.json"
                    ) =>
            {
                true
            }
            ["home", uid, "agent", agent, "session", session, "context", file]
                if uid.parse::<u32>().is_ok()
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
            ["home", uid, "agent", agent, "session", session, "context", cache, "index.jsonl"]
                if uid.parse::<u32>().is_ok()
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
            ["home", uid, "agent", agent, "session", "index", ref tail @ ..]
                if uid.parse::<u32>().is_ok() && is_object_name(agent) =>
            {
                matches!(tail, [] | ["by-cwd" | "by-hash" | "by-uuid"])
            }
            ["home", uid, "agent", agent, "session", session, ref tail @ ..]
                if uid.parse::<u32>().is_ok()
                    && is_object_name(agent)
                    && is_object_name(session) =>
            {
                matches!(
                    tail,
                    []
                        | ["context"]
                        | ["context", "pinned" | "swap" | "dedup" | "child"]
                        | ["context", "swap", "chunk"]
                        | ["context", "dedup", "blob"]
                )
            }
            _ => false,
        }
    }

    fn resolve(&self, abi_path: &str) -> Result<PathBuf, FuseV1Error> {
        resolve_fuse_abi_path(&self.root, abi_path)
    }
}

fn fuse_readlink_error(error: &std::io::Error) -> FuseV1Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => FuseV1Error::NotFound,
        std::io::ErrorKind::PermissionDenied => FuseV1Error::PermissionDenied,
        _ => FuseV1Error::InvalidPath,
    }
}
