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
        if !path.is_dir() {
            return Err(FuseV1Error::NotDirectory);
        }
        let entries = fs::read_dir(&path).map_err(|_error| FuseV1Error::Io)?;
        let mut output = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_error| FuseV1Error::Io)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_error| FuseV1Error::InvalidPath)?;
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|error| fuse_metadata_error(&error))?;
            output.push(FuseV1DirEntry::new(
                name,
                fuse_file_type(metadata.file_type()),
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
        if path.is_dir() {
            return Err(FuseV1Error::NotFile);
        }
        fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                FuseV1Error::NotFound
            } else {
                FuseV1Error::Io
            }
        })
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
        if path.is_dir() {
            return Err(FuseV1Error::NotFile);
        }
        let mut file = fs::File::open(path).map_err(|error| fuse_metadata_error(&error))?;
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
        fs::read_link(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                FuseV1Error::NotFound
            } else {
                FuseV1Error::InvalidPath
            }
        })
    }

    fn virtual_object_attr(&self, abi_path: &str) -> Result<Option<FuseV1Attr>, FuseV1Error> {
        if let Some((file_type, size, mode)) = self.virtual_model_entry(abi_path)? {
            return Ok(Some(FuseV1Attr::with_owner(
                abi_path.to_owned(),
                file_type,
                size,
                mode,
                0,
                0,
            )));
        }
        let Some(content) = self.virtual_object_content(abi_path)? else {
            return Ok(None);
        };
        Ok(Some(FuseV1Attr::with_owner(
            abi_path.to_owned(),
            FuseV1FileType::Regular,
            u64::try_from(content.len()).map_err(|_error| FuseV1Error::Io)?,
            0o555,
            0,
            0,
        )))
    }

    fn virtual_model_readdir(
        &self,
        abi_path: &str,
    ) -> Result<Option<Vec<FuseV1DirEntry>>, FuseV1Error> {
        let mut entries = match abi_path {
            "model" => {
                let mut provider_names = HashSet::from([DEBUG_ECHO_PROVIDER.to_owned()]);
                let model_root = self.root.join("model");
                if model_root.is_dir() {
                    for name in read_model_provider_dirs(&model_root)? {
                        provider_names.insert(name);
                    }
                }
                for provider in projected_provider_models(
                    &self.provider_config_dir,
                    &self.provider_model_cache_dir,
                )?
                    .into_iter()
                    .map(|model| model.provider)
                {
                    provider_names.insert(provider);
                }
                let mut entries = provider_names
                    .into_iter()
                    .map(|provider| FuseV1DirEntry::new(provider, FuseV1FileType::Directory))
                    .collect::<Vec<_>>();
                entries.push(FuseV1DirEntry::new(
                    DEFAULT_MODEL_ALIAS.to_owned(),
                    FuseV1FileType::Symlink,
                ));
                entries.push(FuseV1DirEntry::new(
                    HELPER_MODEL_ALIAS.to_owned(),
                    FuseV1FileType::Symlink,
                ));
                entries
            }
            "model/debug" => vec![
                FuseV1DirEntry::new(DEBUG_ECHO_NAME.to_owned(), FuseV1FileType::Regular),
                FuseV1DirEntry::new(format!("{DEBUG_ECHO_NAME}.d"), FuseV1FileType::Directory),
            ],
            "model/debug/echo.d" => model_control_dir_entries(),
            _ => {
                if let Some(model) =
                    projected_provider_model_control_dir(
                        &self.provider_config_dir,
                        &self.provider_model_cache_dir,
                        abi_path,
                    )?
                {
                    let _ = model;
                    model_control_dir_entries()
                } else if let Some(provider) = abi_path.strip_prefix("model/") {
                    if provider.contains('/') || provider == DEBUG_ECHO_PROVIDER {
                        return Ok(None);
                    }
                    let models = projected_provider_models_for_provider(
                        &self.provider_config_dir,
                        &self.provider_model_cache_dir,
                        provider,
                    )?;
                    if models.is_empty() {
                        return Ok(None);
                    }
                    let mut entries = Vec::new();
                    for model in models {
                        entries.push(FuseV1DirEntry::new(
                            model.model.clone(),
                            FuseV1FileType::Regular,
                        ));
                        entries.push(FuseV1DirEntry::new(
                            format!("{}.d", model.model),
                            FuseV1FileType::Directory,
                        ));
                    }
                    entries
                } else {
                    return Ok(None);
                }
            }
        };
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Some(entries))
    }

    fn virtual_object_content(&self, abi_path: &str) -> Result<Option<String>, FuseV1Error> {
        if let Some(content) = self.virtual_model_content(abi_path)? {
            return Ok(Some(content));
        }
        let Some(object) = self.virtual_exec_object(abi_path) else {
            return Ok(None);
        };
        object_exec_metadata(object.class, &object.name, &object.control_dir).map(Some)
    }

    fn virtual_model_content(&self, abi_path: &str) -> Result<Option<String>, FuseV1Error> {
        if abi_path == "model/debug/echo" {
            return Ok(Some(debug_echo_model_metadata()));
        }
        if let Some(file) = abi_path.strip_prefix("model/debug/echo.d/") {
            return Ok(debug_echo_control_content(file).map(str::to_owned));
        }
        let Some(model) = projected_provider_model_for_exec(
            &self.provider_config_dir,
            &self.provider_model_cache_dir,
            abi_path,
        )?
        else {
            let Some((model, file)) = projected_provider_model_control_file(
                &self.provider_config_dir,
                &self.provider_model_cache_dir,
                abi_path,
            )?
            else {
                return Ok(None);
            };
            return Ok(provider_model_control_content(&model, file));
        };
        Ok(Some(provider_model_metadata(&model)))
    }

    fn virtual_exec_object(&self, abi_path: &str) -> Option<VirtualExecObject> {
        let (class, name) = parse_abi_path(abi_path).executable_object()?;
        let name = name.into_owned();
        let control_dir = self.root.join(class.as_str()).join(format!("{name}.d"));
        if !control_dir.is_dir() {
            return None;
        }
        Some(VirtualExecObject {
            class,
            name,
            control_dir,
        })
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
        if !is_fuse_v1_writable_control_path(&normalized) {
            return Err(FuseV1Error::NotControlFile);
        }
        let path = self.resolve(&normalized)?;
        let content = std::str::from_utf8(content).map_err(|_error| FuseV1Error::InvalidContent)?;
        atomic_replace_text(&path, content).map_err(|_error| FuseV1Error::Io)
    }

    fn resolve(&self, abi_path: &str) -> Result<PathBuf, FuseV1Error> {
        resolve_fuse_abi_path(&self.root, abi_path)
    }

    fn virtual_model_entry(
        &self,
        abi_path: &str,
    ) -> Result<Option<(FuseV1FileType, u64, u32)>, FuseV1Error> {
        match abi_path {
            path if model_alias_name(path).is_some() => Ok(Some((
                FuseV1FileType::Symlink,
                u64::try_from(
                    self.default_model_alias_target(model_alias_name(path).unwrap_or_default())?
                        .as_os_str()
                        .len(),
                )
                .map_err(|_error| FuseV1Error::Io)?,
                0o777,
            ))),
            "model/debug" | "model/debug/echo.d" => Ok(Some((FuseV1FileType::Directory, 0, 0o755))),
            "model/debug/echo" => virtual_regular_entry(&debug_echo_model_metadata(), 0o555),
            path => {
                if let Some(file) = path.strip_prefix("model/debug/echo.d/") {
                    let Some(content) = debug_echo_control_content(file) else {
                        return Ok(None);
                    };
                    return virtual_regular_entry(content, 0o644);
                }
                if projected_provider_models_for_provider_path(
                    &self.provider_config_dir,
                    &self.provider_model_cache_dir,
                    path,
                )?
                .is_some()
                {
                    return Ok(Some((FuseV1FileType::Directory, 0, 0o755)));
                }
                if let Some(model) = projected_provider_model_for_exec(
                    &self.provider_config_dir,
                    &self.provider_model_cache_dir,
                    path,
                )?
                {
                    let content = provider_model_metadata(&model);
                    return virtual_regular_entry(&content, 0o555);
                }
                if projected_provider_model_control_dir(
                    &self.provider_config_dir,
                    &self.provider_model_cache_dir,
                    path,
                )?
                .is_some()
                {
                    return Ok(Some((FuseV1FileType::Directory, 0, 0o755)));
                }
                let Some((model, file)) = projected_provider_model_control_file(
                    &self.provider_config_dir,
                    &self.provider_model_cache_dir,
                    path,
                )?
                else {
                    return Ok(None);
                };
                let Some(content) = provider_model_control_content(&model, file) else {
                    return Ok(None);
                };
                virtual_regular_entry(&content, 0o644)
            }
        }
    }

    fn default_model_alias_target(&self, alias: &str) -> Result<PathBuf, FuseV1Error> {
        let path = self.resolve(&format!("model/{alias}"))?;
        if let Ok(target) = fs::read_link(path)
            && is_valid_ctx_model_symlink(&target)
        {
            return Ok(target);
        }
        Ok(PathBuf::from(DEFAULT_MODEL_ALIAS_TARGET))
    }
}

fn model_alias_name(abi_path: &str) -> Option<&str> {
    let alias = abi_path.strip_prefix("model/")?;
    matches!(alias, DEFAULT_MODEL_ALIAS | HELPER_MODEL_ALIAS).then_some(alias)
}

fn model_control_dir_entries() -> Vec<FuseV1DirEntry> {
    MODEL_CONTROL_FILES
        .iter()
        .map(|file| FuseV1DirEntry::new((*file).to_owned(), FuseV1FileType::Regular))
        .collect()
}

fn virtual_regular_entry(
    content: &str,
    mode: u32,
) -> Result<Option<(FuseV1FileType, u64, u32)>, FuseV1Error> {
    Ok(Some((
        FuseV1FileType::Regular,
        u64::try_from(content.len()).map_err(|_error| FuseV1Error::Io)?,
        mode,
    )))
}

fn projected_metadata_mode(abi_path: &str, metadata: &fs::Metadata) -> u32 {
    let mode = metadata.permissions().mode();
    if abi_path.is_empty() && metadata.is_dir() {
        return (mode & !0o7777) | 0o755;
    }
    mode
}
