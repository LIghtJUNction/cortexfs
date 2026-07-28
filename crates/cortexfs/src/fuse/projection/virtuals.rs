use super::*;

use crate::support::plain::{
    open_plain_directory, path_metadata_no_follow, read_small_text_file, read_symlink_target,
};

impl FuseProjection {
    pub(crate) fn virtual_object_attr(
        &self,
        abi_path: &str,
        snapshot: Option<&ProviderSnapshot>,
    ) -> Result<Option<FuseAttr>, FuseError> {
        if (matches!(abi_path, "model/debug" | "model/debug/echo.d")
            || abi_path.starts_with("model/debug/echo.d/"))
            && self.backing_directory_exists(abi_path, snapshot)?
        {
            return Ok(None);
        }
        let Some(mut file) = self.projected_file(abi_path, snapshot)? else {
            return Ok(None);
        };
        if Self::is_agent_wrapper_path(abi_path) {
            let metadata = fs::symlink_metadata(self.resolve_with_snapshot(abi_path, snapshot)?)
                .map_err(|error| fuse_metadata_error(&error))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(FuseError::InvalidPath);
            }
            file.attr.uid = metadata.uid();
            file.attr.gid = metadata.gid();
        }
        Ok(Some(file.attr))
    }

    pub(crate) fn virtual_model_readdir(
        &self,
        abi_path: &str,
        snapshot: &ProviderSnapshot,
    ) -> Result<Option<Vec<FuseDirEntry>>, FuseError> {
        let mut entries = match abi_path {
            "model" => {
                let mut provider_names = HashSet::from([DEBUG_ECHO_PROVIDER.to_owned()]);
                let model_root = self.root.join("model");
                provider_names.extend(snapshot.active().iter().cloned());
                let mut entries = provider_names
                    .iter()
                    .cloned()
                    .map(|provider| FuseDirEntry::new(provider, FuseFileType::Directory))
                    .collect::<Vec<_>>();
                if fuse_plain_dir_exists(&model_root)? {
                    entries.extend(read_flat_model_entries(&model_root)?.into_iter().filter(
                        |entry| {
                            !provider_names.contains(entry.name())
                                && !is_model_alias(entry.name())
                                && entry.name() != MODEL_ROUTE_FILE
                        },
                    ));
                }
                entries.extend(
                    MODEL_ALIASES
                        .iter()
                        .map(|alias| FuseDirEntry::new((*alias).to_owned(), FuseFileType::Symlink)),
                );
                entries.push(FuseDirEntry::new(
                    MODEL_ROUTE_FILE.to_owned(),
                    FuseFileType::Regular,
                ));
                entries
            }
            "model/debug" => vec![
                FuseDirEntry::new(DEBUG_ECHO_NAME.to_owned(), FuseFileType::Regular),
                FuseDirEntry::new(format!("{DEBUG_ECHO_NAME}.d"), FuseFileType::Directory),
            ],
            "model/debug/echo.d" => self.virtual_model_control_dir_entries(abi_path, snapshot)?,
            _ => {
                if let Some(model) = projected_provider_model_control_dir(snapshot, abi_path) {
                    let _ = model;
                    self.virtual_model_control_dir_entries(abi_path, snapshot)?
                } else if let Some(provider) = abi_path.strip_prefix("model/") {
                    if provider.contains('/') || provider == DEBUG_ECHO_PROVIDER {
                        return Ok(None);
                    }
                    let Some(models) =
                        projected_provider_models_for_provider_path(snapshot, abi_path)
                    else {
                        return Ok(None);
                    };
                    let mut entries = Vec::new();
                    for model in models {
                        entries.push(FuseDirEntry::new(
                            model.model.clone(),
                            FuseFileType::Regular,
                        ));
                        entries.push(FuseDirEntry::new(
                            format!("{}.d", model.model),
                            FuseFileType::Directory,
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

    #[expect(
        clippy::case_sensitive_file_extension_comparisons,
        reason = "the CortexFS ABI reserves the exact lowercase .d suffix"
    )]
    pub(crate) fn hides_model_residue(
        &self,
        abi_path: &str,
        snapshot: Option<&ProviderSnapshot>,
    ) -> Result<bool, FuseError> {
        let Some(path) = abi_path.strip_prefix("model/") else {
            return Ok(false);
        };
        let (provider, tail) = path.split_once('/').unwrap_or((path, ""));
        if provider == DEBUG_ECHO_PROVIDER {
            return Ok(!matches!(
                tail,
                "" | "echo"
                    | "echo.d"
                    | "echo.d/id"
                    | "echo.d/driver"
                    | "echo.d/cap"
                    | "echo.d/effort"
                    | "echo.d/fallback"
                    | "echo.d/limit"
                    | "echo.d/default"
                    | "echo.d/session"
                    | "echo.d/status"
                    | "echo.d/log"
            ));
        }
        if is_model_alias(provider) || provider == MODEL_ROUTE_FILE {
            return Ok(!tail.is_empty());
        }
        if provider.ends_with(".d") || !is_object_name(provider) {
            return Ok(false);
        }
        let snapshot = snapshot.ok_or(FuseError::Io)?;
        let active = snapshot.active().contains(provider);
        if !active {
            return self.physical_model_entry_is_nonfile(provider);
        }
        if tail.is_empty() {
            return Ok(false);
        }
        let models = snapshot
            .models()
            .iter()
            .filter(|model| model.provider == provider)
            .collect::<Vec<_>>();
        let mut parts = tail.split('/');
        let entry = parts.next().unwrap_or_default();
        if let Some(model) = entry.strip_suffix(".d") {
            return Ok(!models.iter().any(|candidate| candidate.model == model));
        }
        Ok(parts.next().is_some() || !models.iter().any(|candidate| candidate.model == entry))
    }

    fn physical_model_entry_is_nonfile(&self, provider: &str) -> Result<bool, FuseError> {
        match fs::symlink_metadata(self.root.join("model").join(provider)) {
            Ok(metadata) => Ok(!metadata.is_file()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(fuse_metadata_error(&error)),
        }
    }

    fn virtual_model_control_dir_entries(
        &self,
        abi_path: &str,
        snapshot: &ProviderSnapshot,
    ) -> Result<Vec<FuseDirEntry>, FuseError> {
        let mut entries = model_control_dir_entries();
        self.append_backing_dir_entries(abi_path, snapshot, &mut entries)?;
        Ok(entries)
    }

    fn append_backing_dir_entries(
        &self,
        abi_path: &str,
        snapshot: &ProviderSnapshot,
        entries: &mut Vec<FuseDirEntry>,
    ) -> Result<(), FuseError> {
        let path = self.resolve_with_snapshot(abi_path, Some(snapshot))?;
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
                .map_err(|_error| FuseError::InvalidPath)?;
            if entries.iter().any(|entry| entry.name() == name) {
                continue;
            }
            let stat = nix::sys::stat::fstatat(
                &directory,
                name.as_str(),
                nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
            entries.push(FuseDirEntry::new(
                name,
                fuse_file_type_from_mode(stat.st_mode),
            ));
        }
        Ok(())
    }

    pub(crate) fn virtual_object_content(
        &self,
        abi_path: &str,
        snapshot: Option<&ProviderSnapshot>,
    ) -> Result<Option<String>, FuseError> {
        Ok(self
            .projected_file(abi_path, snapshot)?
            .and_then(|file| file.content))
    }

    pub(crate) fn virtual_model_content(
        &self,
        abi_path: &str,
        snapshot: &ProviderSnapshot,
    ) -> Result<Option<String>, FuseError> {
        self.virtual_object_content(abi_path, Some(snapshot))
    }

    fn projected_file(
        &self,
        abi_path: &str,
        snapshot: Option<&ProviderSnapshot>,
    ) -> Result<Option<ProjectedFile>, FuseError> {
        let kind = parse_abi_path(abi_path);
        if let Some(file) = self.virtual_model_file(abi_path, kind, snapshot)? {
            return Ok(Some(file));
        }
        let Some((class, name)) = kind.executable_object() else {
            return Ok(None);
        };
        let control_dir = self.root.join(class.as_str()).join(format!("{name}.d"));
        if !fuse_plain_dir_exists(&control_dir).unwrap_or(false) {
            return Ok(None);
        }
        let content = object_exec_metadata(class, &name, &control_dir)?;
        projected_regular_file(abi_path, content, 0o555).map(Some)
    }

    fn virtual_model_file(
        &self,
        abi_path: &str,
        kind: AbiPathKind<'_>,
        snapshot: Option<&ProviderSnapshot>,
    ) -> Result<Option<ProjectedFile>, FuseError> {
        if let Some(alias) = model_alias_name(abi_path) {
            let snapshot = snapshot.ok_or(FuseError::Io)?;
            let size = self
                .default_model_alias_target(alias, snapshot)?
                .as_os_str()
                .len();
            return Ok(Some(ProjectedFile {
                attr: FuseAttr::new(
                    abi_path.to_owned(),
                    FuseFileType::Symlink,
                    u64::try_from(size).map_err(|_error| FuseError::Io)?,
                    0o777,
                ),
                content: None,
            }));
        }
        let directory = || ProjectedFile {
            attr: FuseAttr::new(abi_path.to_owned(), FuseFileType::Directory, 0, 0o755),
            content: None,
        };
        match kind {
            AbiPathKind::ModelRoute => {
                let path = self.resolve_with_snapshot(abi_path, snapshot)?;
                let content = match path_metadata_no_follow(&path) {
                    Ok(metadata) if metadata.is_file() => {
                        read_small_text_file(&path, MAX_FUSE_SMALL_READ_BYTES)
                            .map_err(|_error| FuseError::Io)?
                    }
                    Ok(_metadata) => DEFAULT_MODEL_ROUTE.to_owned(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        DEFAULT_MODEL_ROUTE.to_owned()
                    }
                    Err(_error) => return Err(FuseError::Io),
                };
                projected_regular_file(abi_path, content, 0o644).map(Some)
            }
            AbiPathKind::ModelDir { provider: "debug" } => Ok(Some(directory())),
            AbiPathKind::Unknown if abi_path == "model/debug/echo.d" => Ok(Some(directory())),
            AbiPathKind::ModelDir { .. } => {
                let snapshot = snapshot.ok_or(FuseError::Io)?;
                Ok(
                    projected_provider_models_for_provider_path(snapshot, abi_path)
                        .map(|_models| directory()),
                )
            }
            AbiPathKind::ObjectExec {
                class: ObjectClass::Model,
                ..
            } if abi_path == "model/debug/echo" => {
                projected_regular_file(abi_path, debug_echo_model_metadata(), 0o555).map(Some)
            }
            AbiPathKind::ObjectExec {
                class: ObjectClass::Model,
                ..
            } => {
                let snapshot = snapshot.ok_or(FuseError::Io)?;
                projected_provider_model_for_exec(snapshot, abi_path)
                    .map(|model| provider_model_metadata(&model))
                    .map(|content| projected_regular_file(abi_path, content, 0o555))
                    .transpose()
            }
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
                let snapshot = snapshot.ok_or(FuseError::Io)?;
                let Some((model, file)) = projected_provider_model_control_file(snapshot, abi_path)
                else {
                    return Ok(None);
                };
                let content = provider::projected_control_content(&model, file);
                content
                    .map(|content| projected_regular_file(abi_path, content, 0o444))
                    .transpose()
            }
            AbiPathKind::Unknown => {
                if !abi_path.starts_with("model/") {
                    return Ok(None);
                }
                let snapshot = snapshot.ok_or(FuseError::Io)?;
                Ok(projected_provider_model_control_dir(snapshot, abi_path)
                    .map(|_model| directory()))
            }
            _ => Ok(None),
        }
    }

    fn backing_directory_exists(
        &self,
        abi_path: &str,
        snapshot: Option<&ProviderSnapshot>,
    ) -> Result<bool, FuseError> {
        match fs::symlink_metadata(self.resolve_with_snapshot(abi_path, snapshot)?) {
            Ok(metadata) => Ok(metadata.is_dir() && !metadata.file_type().is_symlink()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(fuse_metadata_error(&error)),
        }
    }

    pub(crate) fn default_model_alias_target(
        &self,
        alias: &str,
        snapshot: &ProviderSnapshot,
    ) -> Result<PathBuf, FuseError> {
        let path = self.resolve_with_snapshot(&format!("model/{alias}"), Some(snapshot))?;
        let existing = read_symlink_target(&path).ok();
        Ok(current_model_alias_target(
            alias,
            existing.as_deref(),
            snapshot,
        ))
    }
}
