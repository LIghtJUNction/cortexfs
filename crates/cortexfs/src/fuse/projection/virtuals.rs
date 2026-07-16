use super::*;

use crate::support::plain::{open_plain_directory, read_small_text_file, read_symlink_target};

impl FuseV1Projection {
    pub(crate) fn virtual_object_attr(
        &self,
        abi_path: &str,
    ) -> Result<Option<FuseV1Attr>, FuseV1Error> {
        if (matches!(abi_path, "model/debug" | "model/debug/echo.d")
            || abi_path.starts_with("model/debug/echo.d/"))
            && self.backing_directory_exists(abi_path)?
        {
            return Ok(None);
        }
        let Some(mut file) = self.projected_file(abi_path)? else {
            return Ok(None);
        };
        if Self::is_agent_wrapper_path(abi_path) {
            let metadata = fs::symlink_metadata(self.resolve(abi_path)?)
                .map_err(|error| fuse_metadata_error(&error))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(FuseV1Error::InvalidPath);
            }
            file.attr.uid = metadata.uid();
            file.attr.gid = metadata.gid();
        }
        Ok(Some(file.attr))
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
                entries.extend(MODEL_ALIASES.iter().map(|alias| {
                    FuseV1DirEntry::new((*alias).to_owned(), FuseV1FileType::Symlink)
                }));
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
        Ok(self.projected_file(abi_path)?.and_then(|file| file.content))
    }

    pub(crate) fn virtual_model_content(
        &self,
        abi_path: &str,
    ) -> Result<Option<String>, FuseV1Error> {
        self.virtual_object_content(abi_path)
    }

    fn projected_file(&self, abi_path: &str) -> Result<Option<ProjectedFile>, FuseV1Error> {
        let kind = parse_abi_path(abi_path);
        if let Some(file) = self.virtual_model_file(abi_path, kind)? {
            return Ok(Some(file));
        }
        let Some((class, name)) = kind.executable_object() else {
            return Ok(None);
        };
        let control_dir = self.root.join(class.as_str()).join(format!("{name}.d"));
        if !fuse_v1_plain_dir_exists(&control_dir).unwrap_or(false) {
            return Ok(None);
        }
        let content = object_exec_metadata(class, &name, &control_dir)?;
        projected_regular_file(abi_path, content, 0o555).map(Some)
    }

    fn virtual_model_file(
        &self,
        abi_path: &str,
        kind: AbiPathKind<'_>,
    ) -> Result<Option<ProjectedFile>, FuseV1Error> {
        if let Some(alias) = model_alias_name(abi_path) {
            let size = self.default_model_alias_target(alias)?.as_os_str().len();
            return Ok(Some(ProjectedFile {
                attr: FuseV1Attr::new(
                    abi_path.to_owned(),
                    FuseV1FileType::Symlink,
                    u64::try_from(size).map_err(|_error| FuseV1Error::Io)?,
                    0o777,
                ),
                content: None,
            }));
        }
        let directory = || ProjectedFile {
            attr: FuseV1Attr::new(abi_path.to_owned(), FuseV1FileType::Directory, 0, 0o755),
            content: None,
        };
        match kind {
            AbiPathKind::ModelRoute => {
                let path = self.resolve(abi_path)?;
                let content = match read_small_text_file(&path, MAX_FUSE_V1_SMALL_READ_BYTES) {
                    Ok(content) => content,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        DEFAULT_MODEL_ROUTE.to_owned()
                    }
                    Err(_error) => return Err(FuseV1Error::Io),
                };
                projected_regular_file(abi_path, content, 0o644).map(Some)
            }
            AbiPathKind::ModelDir { provider: "debug" } => Ok(Some(directory())),
            AbiPathKind::Unknown if abi_path == "model/debug/echo.d" => Ok(Some(directory())),
            AbiPathKind::ModelDir { .. } => Ok(projected_provider_models_for_provider_path(
                &self.provider_config_dir,
                &self.provider_model_cache_dir,
                abi_path,
            )?
            .map(|_models| directory())),
            AbiPathKind::ObjectExec {
                class: ObjectClass::Model,
                ..
            } if abi_path == "model/debug/echo" => {
                projected_regular_file(abi_path, debug_echo_model_metadata(), 0o555).map(Some)
            }
            AbiPathKind::ObjectExec {
                class: ObjectClass::Model,
                ..
            } => projected_provider_model_for_exec(
                &self.provider_config_dir,
                &self.provider_model_cache_dir,
                abi_path,
            )?
            .map(|model| provider_model_metadata(&model))
            .map(|content| projected_regular_file(abi_path, content, 0o555))
            .transpose(),
            AbiPathKind::ObjectControl {
                class: ObjectClass::Model,
                ..
            } if abi_path.starts_with("model/debug/echo.d/") => {
                let Some(content) = abi_path
                    .strip_prefix("model/debug/echo.d/")
                    .and_then(|file| debug_model_control_content(DEBUG_ECHO_MODEL, file))
                else {
                    return Ok(None);
                };
                projected_regular_file(abi_path, content, 0o644).map(Some)
            }
            AbiPathKind::ObjectControl {
                class: ObjectClass::Model,
                ..
            } => {
                let Some((model, file)) = projected_provider_model_control_file(
                    &self.provider_config_dir,
                    &self.provider_model_cache_dir,
                    abi_path,
                )?
                else {
                    return Ok(None);
                };
                let content = self
                    .backing_control_content(abi_path)?
                    .or_else(|| provider_model_control_content(&model, file));
                content
                    .map(|content| projected_regular_file(abi_path, content, 0o644))
                    .transpose()
            }
            AbiPathKind::Unknown => Ok(projected_provider_model_control_dir(
                &self.provider_config_dir,
                &self.provider_model_cache_dir,
                abi_path,
            )?
            .map(|_model| directory())),
            _ => Ok(None),
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
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_error) => Err(FuseV1Error::Io),
            Ok(metadata) if metadata.is_dir() => Ok(None),
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                Err(FuseV1Error::Io)
            }
            Ok(_metadata) => match read_small_text_file(&path, MAX_FUSE_V1_SMALL_READ_BYTES) {
                Ok(content) => Ok(Some(content)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_error) => Err(FuseV1Error::Io),
            },
        }
    }

    pub(crate) fn default_model_alias_target(&self, alias: &str) -> Result<PathBuf, FuseV1Error> {
        let path = self.resolve(&format!("model/{alias}"))?;
        if let Ok(target) = read_symlink_target(&path)
            && is_valid_ctx_model_symlink(&target)
        {
            return Ok(target);
        }
        if alias == HELPER_MODEL_ALIAS {
            return Ok(PathBuf::from(HELPER_MODEL_ALIAS_TARGET));
        }
        if alias != DEFAULT_MODEL_ALIAS {
            return self.default_model_alias_target(DEFAULT_MODEL_ALIAS);
        }
        Ok(PathBuf::from(DEFAULT_MODEL_ALIAS_TARGET))
    }
}
