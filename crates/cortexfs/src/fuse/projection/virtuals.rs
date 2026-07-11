use super::*;

use crate::support::plain::{open_plain_directory, read_small_text_file, read_symlink_target};

impl FuseV1Projection {
    pub(crate) fn virtual_object_attr(
        &self,
        abi_path: &str,
    ) -> Result<Option<FuseV1Attr>, FuseV1Error> {
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
        let (uid, gid) = if Self::is_agent_wrapper_path(abi_path) {
            let metadata = fs::symlink_metadata(self.resolve(abi_path)?)
                .map_err(|error| fuse_metadata_error(&error))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(FuseV1Error::InvalidPath);
            }
            (metadata.uid(), metadata.gid())
        } else {
            (0, 0)
        };
        Ok(Some(FuseV1Attr::with_owner(
            abi_path.to_owned(),
            FuseV1FileType::Regular,
            u64::try_from(content.len()).map_err(|_error| FuseV1Error::Io)?,
            0o555,
            uid,
            gid,
        )))
    }

    pub(crate) fn virtual_model_readdir(
        &self,
        abi_path: &str,
    ) -> Result<Option<Vec<FuseV1DirEntry>>, FuseV1Error> {
        let mut entries = match abi_path {
            "model" => {
                let mut provider_names = HashSet::from([DEBUG_ECHO_PROVIDER.to_owned()]);
                let model_root = self.root.join("model");
                if fuse_v1_plain_dir_exists(&model_root)? {
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
                entries.push(FuseV1DirEntry::new(
                    MODEL_ROUTE_FILE.to_owned(),
                    FuseV1FileType::Regular,
                ));
                entries
            }
            "model/debug" => vec![
                FuseV1DirEntry::new(DEBUG_ECHO_NAME.to_owned(), FuseV1FileType::Regular),
                FuseV1DirEntry::new(format!("{DEBUG_ECHO_NAME}.d"), FuseV1FileType::Directory),
            ],
            "model/debug/echo.d" => self.virtual_model_control_dir_entries(abi_path)?,
            _ => {
                if let Some(model) = projected_provider_model_control_dir(
                    &self.provider_config_dir,
                    &self.provider_model_cache_dir,
                    abi_path,
                )? {
                    let _ = model;
                    self.virtual_model_control_dir_entries(abi_path)?
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

    fn virtual_model_control_dir_entries(
        &self,
        abi_path: &str,
    ) -> Result<Vec<FuseV1DirEntry>, FuseV1Error> {
        let mut entries = model_control_dir_entries();
        self.append_backing_dir_entries(abi_path, &mut entries)?;
        Ok(entries)
    }

    fn append_backing_dir_entries(
        &self,
        abi_path: &str,
        entries: &mut Vec<FuseV1DirEntry>,
    ) -> Result<(), FuseV1Error> {
        let path = self.resolve(abi_path)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(fuse_metadata_error(&error)),
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Ok(());
        }
        let directory = open_plain_directory(&path).map_err(|error| fuse_metadata_error(&error))?;
        for entry in fs::read_dir(support::plain::proc_fd_path(&directory))
            .map_err(|error| fuse_metadata_error(&error))?
        {
            let entry = entry.map_err(|error| fuse_metadata_error(&error))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_error| FuseV1Error::InvalidPath)?;
            if entries.iter().any(|entry| entry.name() == name) {
                continue;
            }
            let stat = nix::sys::stat::fstatat(
                &directory,
                name.as_str(),
                nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
            entries.push(FuseV1DirEntry::new(
                name,
                fuse_file_type_from_mode(stat.st_mode),
            ));
        }
        Ok(())
    }

    pub(crate) fn virtual_object_content(
        &self,
        abi_path: &str,
    ) -> Result<Option<String>, FuseV1Error> {
        if let Some(content) = self.virtual_model_content(abi_path)? {
            return Ok(Some(content));
        }
        let Some(object) = self.virtual_exec_object(abi_path) else {
            return Ok(None);
        };
        object_exec_metadata(object.class, &object.name, &object.control_dir).map(Some)
    }

    fn virtual_model_content(&self, abi_path: &str) -> Result<Option<String>, FuseV1Error> {
        if abi_path == format!("model/{MODEL_ROUTE_FILE}") {
            let path = self.resolve(abi_path)?;
            return match read_small_text_file(&path, MAX_FUSE_V1_SMALL_READ_BYTES) {
                Ok(content) => Ok(Some(content)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(Some(DEFAULT_MODEL_ROUTE.to_owned()))
                }
                Err(_error) => Err(FuseV1Error::Io),
            };
        }
        if abi_path == "model/debug/echo" {
            return Ok(Some(debug_echo_model_metadata()));
        }
        if let Some(file) = abi_path.strip_prefix("model/debug/echo.d/") {
            return Ok(debug_model_control_content(DEBUG_ECHO_MODEL, file));
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
            if let Some(content) = self.backing_control_content(abi_path)? {
                return Ok(Some(content));
            }
            return Ok(provider_model_control_content(&model, file));
        };
        Ok(Some(provider_model_metadata(&model)))
    }

    fn virtual_exec_object(&self, abi_path: &str) -> Option<VirtualExecObject> {
        let (class, name) = parse_abi_path(abi_path).executable_object()?;
        let name = name.into_owned();
        let control_dir = self.root.join(class.as_str()).join(format!("{name}.d"));
        if !fuse_v1_plain_dir_exists(&control_dir).ok()? {
            return None;
        }
        Some(VirtualExecObject {
            class,
            name,
            control_dir,
        })
    }

    fn virtual_model_entry(
        &self,
        abi_path: &str,
    ) -> Result<Option<(FuseV1FileType, u64, u32)>, FuseV1Error> {
        match abi_path {
            path if path == format!("model/{MODEL_ROUTE_FILE}") => {
                let content = self.virtual_model_content(path)?.unwrap_or_default();
                virtual_regular_entry(&content, 0o644)
            }
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
            "model/debug" | "model/debug/echo.d" if !self.backing_directory_exists(abi_path)? => {
                Ok(Some((FuseV1FileType::Directory, 0, 0o755)))
            }
            "model/debug/echo" => virtual_regular_entry(&debug_echo_model_metadata(), 0o555),
            path => {
                if path.starts_with("model/debug/echo.d/") && self.backing_directory_exists(path)? {
                    return Ok(None);
                }
                if let Some(file) = path.strip_prefix("model/debug/echo.d/") {
                    let Some(content) = debug_model_control_content(DEBUG_ECHO_MODEL, file) else {
                        return Ok(None);
                    };
                    return virtual_regular_entry(&content, 0o644);
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
                )? {
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
                if let Some(content) = self.backing_control_content(path)? {
                    return virtual_regular_entry(&content, 0o644);
                }
                let Some(content) = provider_model_control_content(&model, file) else {
                    return Ok(None);
                };
                virtual_regular_entry(&content, 0o644)
            }
        }
    }

    fn backing_directory_exists(&self, abi_path: &str) -> Result<bool, FuseV1Error> {
        match fs::symlink_metadata(self.resolve(abi_path)?) {
            Ok(metadata) => Ok(metadata.is_dir() && !metadata.file_type().is_symlink()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(fuse_metadata_error(&error)),
        }
    }

    fn backing_control_content(&self, abi_path: &str) -> Result<Option<String>, FuseV1Error> {
        let path = self.resolve(abi_path)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_error) => return Err(FuseV1Error::Io),
        };
        if metadata.is_dir() {
            return Ok(None);
        }
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(FuseV1Error::Io);
        }
        match read_small_text_file(&path, MAX_FUSE_V1_SMALL_READ_BYTES) {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_error) => Err(FuseV1Error::Io),
        }
    }

    pub(crate) fn default_model_alias_target(&self, alias: &str) -> Result<PathBuf, FuseV1Error> {
        let path = self.resolve(&format!("model/{alias}"))?;
        if let Ok(target) = read_symlink_target(&path)
            && is_valid_ctx_model_symlink(&target)
        {
            return Ok(target);
        }
        Ok(PathBuf::from(if alias == HELPER_MODEL_ALIAS {
            HELPER_MODEL_ALIAS_TARGET
        } else {
            DEFAULT_MODEL_ALIAS_TARGET
        }))
    }
}
