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
        if offset != 0 {
            return Err(FuseV1Error::InvalidOffset);
        }
        if content.len() > MAX_FUSE_V1_SMALL_WRITE_BYTES {
            return Err(FuseV1Error::TooLarge);
        }
        let normalized = normalize_fuse_abi_path(abi_path)?;
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
