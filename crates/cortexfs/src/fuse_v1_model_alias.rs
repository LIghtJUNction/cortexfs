impl FuseV1Projection {
    /// Persists a temporary model symlink used by atomic alias replacement.
    pub fn set_model_alias_symlink(
        &self,
        abi_path: &str,
        target: &Path,
    ) -> Result<FuseV1Node, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !is_model_alias_symlink_path(&normalized) {
            return Err(FuseV1Error::InvalidPath);
        }
        let target = normalize_model_alias_target(target).ok_or(FuseV1Error::InvalidPath)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseV1Error::InvalidPath)?;
        fs::create_dir_all(parent).map_err(|_error| FuseV1Error::Io)?;
        symlink(&target, &path).map_err(|_error| FuseV1Error::Io)?;
        model_alias_symlink_node(normalized, &target)
    }

    /// Persists a model alias symlink such as `model/main`.
    pub fn set_model_alias(&self, abi_path: &str, target: &Path) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let Some(alias) = model_alias_name(&normalized) else {
            return Err(FuseV1Error::NotControlFile);
        };
        let target = normalize_model_alias_target(target).ok_or(FuseV1Error::InvalidPath)?;
        let path = self.resolve(&format!("model/{alias}"))?;
        let parent = path.parent().ok_or(FuseV1Error::InvalidPath)?;
        fs::create_dir_all(parent).map_err(|_error| FuseV1Error::Io)?;
        let temporary = parent.join(format!(".{alias}.tmp.{}", std::process::id()));
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_error) => return Err(FuseV1Error::Io),
        }
        symlink(&target, &temporary).map_err(|_error| FuseV1Error::Io)?;
        fs::rename(&temporary, &path).map_err(|_error| FuseV1Error::Io)
    }

    /// Removes a persisted model alias override, restoring the built-in target.
    pub fn remove_model_alias(&self, abi_path: &str) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if model_alias_name(&normalized).is_none() {
            return Err(FuseV1Error::NotControlFile);
        }
        match fs::remove_file(self.resolve(&normalized)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_error) => Err(FuseV1Error::Io),
        }
    }

    /// Renames a temporary model symlink onto `model/main` or `model/helper`.
    pub fn rename_model_alias_symlink(&self, from: &str, to: &str) -> Result<(), FuseV1Error> {
        let from = normalize_fuse_abi_path(from)?;
        let to = normalize_fuse_abi_path(to)?;
        if !is_model_alias_symlink_path(&from) || model_alias_name(&to).is_none() {
            return Err(FuseV1Error::InvalidPath);
        }
        let source = self.resolve(&from)?;
        let target = fs::read_link(&source).map_err(|error| fuse_metadata_error(&error))?;
        self.set_model_alias(&to, &target)?;
        if from != to {
            match fs::remove_file(source) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_error) => return Err(FuseV1Error::Io),
            }
        }
        Ok(())
    }
}

fn normalize_model_alias_target(target: &Path) -> Option<PathBuf> {
    let raw = target.to_str()?;
    let model = raw.strip_prefix("/ctx/model/").unwrap_or(raw);
    is_model_name(model).then(|| PathBuf::from(format!("/ctx/model/{model}")))
}

fn is_model_alias_symlink_path(abi_path: &str) -> bool {
    let Some(name) = abi_path.strip_prefix("model/") else {
        return false;
    };
    !name.contains('/') && is_object_name(name)
}

fn model_alias_symlink_node(
    abi_path: String,
    target: &Path,
) -> Result<FuseV1Node, FuseV1Error> {
    let size = u64::try_from(target.as_os_str().len()).map_err(|_error| FuseV1Error::Io)?;
    Ok(FuseV1Node::new(
        fuse_v1_inode_for_path(&abi_path),
        abi_path.clone(),
        FuseV1Attr::with_owner(abi_path, FuseV1FileType::Symlink, size, 0o777, 0, 0),
    ))
}
