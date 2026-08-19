use crate::*;

pub(crate) fn debug_echo_model_metadata() -> String {
    debug_model_metadata("debug/echo", "Built-in debug echo model", "chat,stream")
}

pub(crate) fn debug_model_metadata(id: &str, description: &str, cap: &str) -> String {
    [
        format!("#!{CORTEXFS_OBJECT_RUNNER}"),
        "# cortexfs.object=model".to_owned(),
        format!("# cortexfs.id={id}"),
        format!("# cortexfs.name={id}"),
        format!("# cortexfs.description={description}"),
        "# cortexfs.type=debug".to_owned(),
        "# cortexfs.created_at=".to_owned(),
        "# cortexfs.owned_by=cortexfs".to_owned(),
        "# cortexfs.context_length=unknown".to_owned(),
        "# cortexfs.context_recommended=unknown".to_owned(),
        "# cortexfs.context_compact=unknown".to_owned(),
        "# cortexfs.driver=debug".to_owned(),
        "# cortexfs.driver.default=debug".to_owned(),
        "# cortexfs.driver.exec=debug".to_owned(),
        "# cortexfs.driver.socket=".to_owned(),
        "# cortexfs.driver.agent=debug".to_owned(),
        "# cortexfs.session=none".to_owned(),
        "# cortexfs.status=idle".to_owned(),
        format!("# cortexfs.cap={cap}"),
    ]
    .join("\n")
        + "\n"
}

pub(crate) fn debug_model_control_content(model: &str, file: &str) -> Option<String> {
    match file {
        "id" => Some(format!("{model}\n")),
        "metadata.json" => Some(
            serde_json::json!({
                "schema": cortexfs_metadatas::MODEL_METADATA_SCHEMA,
                "metadata": {"provider": "debug", "id": model, "name": model},
            })
            .to_string()
                + "\n",
        ),
        "driver" => Some("default=debug\nexec=debug\nagent=debug\n".to_owned()),
        "cap" => Some("chat\nstream\n".to_owned()),
        "effort" => Some("auto\n".to_owned()),
        "limit" | "recommended" | "compact" => Some("unknown\n".to_owned()),
        "default" | "log" => Some("\n".to_owned()),
        "session" => Some("none\n".to_owned()),
        "status" => Some("idle\n".to_owned()),
        _ => None,
    }
}

pub(crate) const fn provider_error(error: provider::ProviderError) -> FuseError {
    match error {
        provider::ProviderError::Invalid => FuseError::InvalidContent,
        provider::ProviderError::Io => FuseError::Io,
    }
}

pub(crate) fn projected_provider_models_for_provider_path(
    snapshot: &ProviderSnapshot,
    abi_path: &str,
) -> Option<Vec<ProjectedProviderModel>> {
    let provider = abi_path.strip_prefix("model/")?;
    if provider.contains('/') || provider == DEBUG_ECHO_PROVIDER || !is_object_name(provider) {
        return None;
    }
    snapshot.active().contains(provider).then(|| {
        snapshot
            .models()
            .iter()
            .filter(|model| model.provider == provider)
            .cloned()
            .collect()
    })
}

pub(crate) fn projected_provider_model_for_exec(
    snapshot: &ProviderSnapshot,
    abi_path: &str,
) -> Option<ProjectedProviderModel> {
    let model_name = model_exec_name(abi_path)?;
    snapshot
        .models()
        .iter()
        .find(|model| format!("{}/{}", model.provider, model.model) == model_name)
        .cloned()
}

pub(crate) fn projected_provider_model_control_dir(
    snapshot: &ProviderSnapshot,
    abi_path: &str,
) -> Option<ProjectedProviderModel> {
    let model_name = abi_path
        .strip_prefix("model/")
        .and_then(|path| path.strip_suffix(".d"))?;
    if !is_model_name(model_name) {
        return None;
    }
    snapshot
        .models()
        .iter()
        .find(|model| format!("{}/{}", model.provider, model.model) == model_name)
        .cloned()
}

pub(crate) fn projected_provider_model_control_file<'a>(
    snapshot: &ProviderSnapshot,
    abi_path: &'a str,
) -> Option<(ProjectedProviderModel, &'a str)> {
    let (dir, file) = abi_path.rsplit_once('/')?;
    let model = projected_provider_model_control_dir(snapshot, dir)?;
    Some((model, file))
}

pub(crate) fn provider_model_metadata(model: &ProjectedProviderModel) -> String {
    let name = format!("{}/{}", model.provider, model.model);
    let routes = parse_model_driver_routes(&model.driver).unwrap_or_default();
    let driver = routes
        .primary_driver_for(ModelDriverUseCase::Default)
        .unwrap_or("openai-chat");
    format!(
        "#!{CORTEXFS_OBJECT_RUNNER}\n\
         # cortexfs.object=model\n\
         # cortexfs.id={name}\n\
         # cortexfs.name={name}\n\
         # cortexfs.description=Configured provider model\n\
         # cortexfs.type=chat\n\
         # cortexfs.created_at=\n\
         # cortexfs.owned_by={}\n\
         # cortexfs.context_length={}\n\
         # cortexfs.context_recommended={}\n\
         # cortexfs.context_compact={}\n\
         # cortexfs.driver={driver}\n\
         # cortexfs.driver.default={}\n\
         # cortexfs.driver.exec={}\n\
         # cortexfs.driver.socket={}\n\
         # cortexfs.driver.agent={}\n\
         # cortexfs.session=none\n\
         # cortexfs.status=configured\n\
         # cortexfs.cap={}\n",
        model.provider,
        model.limit,
        model.recommended,
        model.compact,
        routes.route_value(ModelDriverUseCase::Default),
        routes.route_value(ModelDriverUseCase::Exec),
        routes.route_value(ModelDriverUseCase::Socket),
        routes.route_value(ModelDriverUseCase::Agent),
        model.cap.lines().collect::<Vec<_>>().join(",")
    )
}

pub(crate) fn read_flat_model_entries(model_root: &Path) -> Result<Vec<FuseDirEntry>, FuseError> {
    let directory = open_plain_directory(model_root).map_err(|_error| FuseError::Io)?;
    let entries =
        fs::read_dir(support::plain::proc_fd_path(&directory)).map_err(|_error| FuseError::Io)?;
    let mut flat = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| FuseError::Io)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_error| FuseError::InvalidPath)?;
        let stat = nix::sys::stat::fstatat(
            &directory,
            name.as_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
        let kind = stat.st_mode & libc::S_IFMT;
        let is_flat_model = kind == libc::S_IFREG && is_object_name(&name);
        let is_flat_control =
            kind == libc::S_IFDIR && name.strip_suffix(".d").is_some_and(is_object_name);
        if is_flat_model || is_flat_control {
            flat.push(FuseDirEntry::new(
                name,
                fuse_file_type_from_mode(stat.st_mode),
            ));
        }
    }
    flat.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(flat)
}
