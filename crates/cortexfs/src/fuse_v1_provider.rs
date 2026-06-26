fn debug_echo_model_metadata() -> String {
    debug_model_metadata(
        "debug/echo",
        "Built-in debug echo model",
        "chat,stream",
    )
}

fn debug_model_metadata(id: &str, description: &str, cap: &str) -> String {
    [
        format!("#!{CORTEXFS_OBJECT_RUNNER}"),
        "# cortexfs.object=model".to_owned(),
        format!("# cortexfs.id={id}"),
        format!("# cortexfs.name={id}"),
        format!("# cortexfs.description={description}"),
        "# cortexfs.type=debug".to_owned(),
        "# cortexfs.created_at=".to_owned(),
        "# cortexfs.owned_by=cortexfs".to_owned(),
        "# cortexfs.context_length=0".to_owned(),
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

fn debug_model_control_content(model: &str, file: &str) -> Option<String> {
    match file {
        "id" => Some(format!("{model}\n")),
        "driver" => Some("default=debug\nexec=debug\nagent=debug\n".to_owned()),
        "cap" => Some("chat\nstream\n".to_owned()),
        "effort" => Some("auto\n".to_owned()),
        "default" | "fallback" | "log" => Some("\n".to_owned()),
        "session" => Some("none\n".to_owned()),
        "status" => Some("idle\n".to_owned()),
        _ => None,
    }
}

fn default_provider_enabled() -> bool {
    true
}

fn projected_provider_models(
    config_dir: &Path,
    cache_dir: &Path,
) -> Result<Vec<ProjectedProviderModel>, FuseV1Error> {
    let configs = read_provider_configs(config_dir)?;
    let mut projected = Vec::new();
    let mut seen = HashSet::new();
    for entry in configs {
        let config = entry.config;
        if !config.enabled {
            continue;
        }
        let provider = projected_provider_name(&config)?;
        let driver = provider_driver_route_table(&config.formats);
        let cap = provider_capability_text(&config.formats);
        for model in provider_config_models(&config, cache_dir, &provider) {
            let key = format!("{provider}/{model}");
            if seen.insert(key) {
                projected.push(ProjectedProviderModel {
                    provider: provider.clone(),
                    model,
                    base_url: normalize_provider_base_url(&config.base_url),
                    driver: driver.clone(),
                    cap: cap.clone(),
                    effort: "auto".to_owned(),
                    fallback: default_provider_model_fallback(
                        &provider,
                        config.default_model.as_deref(),
                    ),
                });
            }
        }
    }
    projected.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.model.cmp(&right.model))
    });
    Ok(projected)
}

struct ProviderConfigEntry {
    config: ProviderConfig,
}

fn read_provider_configs(config_dir: &Path) -> Result<Vec<ProviderConfigEntry>, FuseV1Error> {
    let entries = match fs::read_dir(config_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_error) => return Err(FuseV1Error::Io),
    };
    let mut configs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| FuseV1Error::Io)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if metadata.file_type().is_dir() {
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if extension != "json" {
            continue;
        }
        let content = fs::read_to_string(&path).map_err(|_error| FuseV1Error::Io)?;
        let Ok(config) = serde_json::from_str::<ProviderConfig>(&content) else {
            continue;
        };
        configs.push(ProviderConfigEntry { config });
    }
    Ok(configs)
}

fn projected_provider_models_for_provider(
    config_dir: &Path,
    cache_dir: &Path,
    provider: &str,
) -> Result<Vec<ProjectedProviderModel>, FuseV1Error> {
    Ok(projected_provider_models(config_dir, cache_dir)?
        .into_iter()
        .filter(|model| model.provider == provider)
        .collect())
}

fn projected_provider_models_for_provider_path(
    config_dir: &Path,
    cache_dir: &Path,
    abi_path: &str,
) -> Result<Option<Vec<ProjectedProviderModel>>, FuseV1Error> {
    let Some(provider) = abi_path.strip_prefix("model/") else {
        return Ok(None);
    };
    if provider.contains('/') || provider == DEBUG_ECHO_PROVIDER {
        return Ok(None);
    }
    let models = projected_provider_models_for_provider(config_dir, cache_dir, provider)?;
    if models.is_empty() {
        Ok(None)
    } else {
        Ok(Some(models))
    }
}

fn projected_provider_model_for_exec(
    config_dir: &Path,
    cache_dir: &Path,
    abi_path: &str,
) -> Result<Option<ProjectedProviderModel>, FuseV1Error> {
    let Some(model_name) = model_exec_name(abi_path) else {
        return Ok(None);
    };
    Ok(projected_provider_models(config_dir, cache_dir)?
        .into_iter()
        .find(|model| format!("{}/{}", model.provider, model.model) == model_name))
}

fn projected_provider_model_control_dir(
    config_dir: &Path,
    cache_dir: &Path,
    abi_path: &str,
) -> Result<Option<ProjectedProviderModel>, FuseV1Error> {
    let Some(model_name) = abi_path
        .strip_prefix("model/")
        .and_then(|path| path.strip_suffix(".d"))
    else {
        return Ok(None);
    };
    if !is_model_name(model_name) {
        return Ok(None);
    }
    Ok(projected_provider_models(config_dir, cache_dir)?
        .into_iter()
        .find(|model| format!("{}/{}", model.provider, model.model) == model_name))
}

fn projected_provider_model_control_file<'a>(
    config_dir: &Path,
    cache_dir: &Path,
    abi_path: &'a str,
) -> Result<Option<(ProjectedProviderModel, &'a str)>, FuseV1Error> {
    let Some((dir, file)) = abi_path.rsplit_once('/') else {
        return Ok(None);
    };
    let Some(model) = projected_provider_model_control_dir(config_dir, cache_dir, dir)? else {
        return Ok(None);
    };
    Ok(Some((model, file)))
}

fn provider_config_models(config: &ProviderConfig, cache_dir: &Path, provider: &str) -> Vec<String> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    if let Some(model) = config.default_model.as_deref() {
        append_provider_model_name(model, &mut models, &mut seen);
    }
    for model in &config.models {
        append_provider_model_name(model, &mut models, &mut seen);
    }
    for model in provider_cached_models(cache_dir, provider) {
        append_provider_model_name(&model, &mut models, &mut seen);
    }
    models
}

fn append_provider_model_name(model: &str, models: &mut Vec<String>, seen: &mut HashSet<String>) {
    let model = model.trim();
    if !is_object_name(model) {
        return;
    }
    if seen.insert(model.to_owned()) {
        models.push(model.to_owned());
    }
}

fn projected_provider_name(config: &ProviderConfig) -> Result<String, FuseV1Error> {
    provider_name_from_config(&config.base_url, config.name.as_deref())
        .map_err(|_error| FuseV1Error::InvalidContent)
}

fn normalize_provider_base_url(base_url: &str) -> String {
    base_url.trim().to_owned()
}

fn provider_driver_route_table(formats: &[String]) -> String {
    let drivers = provider_drivers(formats);
    let default = drivers
        .iter()
        .find(|driver| driver.as_str() == "openai-chat")
        .or_else(|| drivers.first())
        .map_or("openai-chat", String::as_str);
    let agent = if drivers.iter().any(|driver| driver == "openai-responses")
        && drivers.iter().any(|driver| driver == "openai-chat")
    {
        "openai-responses,openai-chat".to_owned()
    } else {
        default.to_owned()
    };
    format!("default={default}\nexec={default}\nagent={agent}\n")
}

fn provider_drivers(formats: &[String]) -> Vec<String> {
    let mut drivers = Vec::new();
    let mut seen = HashSet::new();
    for format in formats {
        let driver = match format.trim() {
            "openai.responses" => "openai-responses",
            "openai.chat" | "openai-compatible" => "openai-chat",
            _ => continue,
        };
        if seen.insert(driver) {
            drivers.push(driver.to_owned());
        }
    }
    if drivers.is_empty() {
        drivers.push("openai-chat".to_owned());
    }
    drivers
}

fn provider_capability_text(formats: &[String]) -> String {
    let mut capabilities = vec!["chat", "stream"];
    if formats
        .iter()
        .any(|format| format.trim() == "openai.responses")
    {
        capabilities.push("tool_call_syntax");
    }
    capabilities.join("\n") + "\n"
}

fn provider_model_metadata(model: &ProjectedProviderModel) -> String {
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
         # cortexfs.context_length=0\n\
         # cortexfs.driver={driver}\n\
         # cortexfs.driver.default={}\n\
         # cortexfs.driver.exec={}\n\
         # cortexfs.driver.socket={}\n\
         # cortexfs.driver.agent={}\n\
         # cortexfs.session=none\n\
         # cortexfs.status=configured\n\
         # cortexfs.cap={}\n",
        model.provider,
        routes.route_value(ModelDriverUseCase::Default),
        routes.route_value(ModelDriverUseCase::Exec),
        routes.route_value(ModelDriverUseCase::Socket),
        routes.route_value(ModelDriverUseCase::Agent),
        model.cap.lines().collect::<Vec<_>>().join(",")
    )
}

fn provider_model_control_content(model: &ProjectedProviderModel, file: &str) -> Option<String> {
    match file {
        "id" => Some(format!("{}/{}\n", model.provider, model.model)),
        "driver" => Some(model.driver.clone()),
        "cap" => Some(model.cap.clone()),
        "effort" => Some(format!("{}\n", model.effort)),
        "fallback" => Some(model.fallback.clone()),
        "default" => Some(format!("base_url={}\n", model.base_url)),
        "session" => Some("none\n".to_owned()),
        "status" => Some("configured\n".to_owned()),
        "log" => Some("\n".to_owned()),
        _ => None,
    }
}

fn default_provider_model_fallback(provider: &str, default_model: Option<&str>) -> String {
    let requested = [
        "gpt-5.5",
        "codex-auto-review",
        "gpt-5.3-codex-spark",
        "gpt-5.4",
        "gpt-5.4-mini",
    ];
    let mut fallback = String::new();
    for model in requested {
        if default_model == Some(model) {
            continue;
        }
        if is_object_name(model) {
            fallback.push_str(provider);
            fallback.push('/');
            fallback.push_str(model);
            fallback.push('\n');
        }
    }
    fallback
}

fn read_model_provider_dirs(model_root: &Path) -> Result<Vec<String>, FuseV1Error> {
    let entries = fs::read_dir(model_root).map_err(|_error| FuseV1Error::Io)?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| FuseV1Error::Io)?;
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_error| FuseV1Error::InvalidPath)?;
        if is_object_name(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}
