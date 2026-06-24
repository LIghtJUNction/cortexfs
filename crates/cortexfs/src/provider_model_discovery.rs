pub fn refresh_provider_model_cache(config_dir: &Path, cache_dir: &Path) -> Result<(), FuseV1Error> {
    fs::create_dir_all(cache_dir).map_err(|_error| FuseV1Error::Io)?;
    for config in read_provider_configs(config_dir)? {
        if !config.config.enabled {
            continue;
        }
        let Some(provider) = provider_name_from_base_url(&config.config.base_url) else {
            continue;
        };
        let Some(api_key) = provider_api_key(&config.config, &provider) else {
            continue;
        };
        let Ok(models) = fetch_provider_models(&config.config.base_url, &api_key) else {
            continue;
        };
        let models = provider_model_names(models);
        if models.is_empty() {
            continue;
        }
        let content = serde_json::json!({ "models": models }).to_string() + "\n";
        atomic_replace_text(&provider_model_cache_path(cache_dir, &provider), &content)
            .map_err(|_error| FuseV1Error::Io)?;
    }
    Ok(())
}

fn provider_cached_models(cache_dir: &Path, provider: &str) -> Vec<String> {
    let Ok(content) = fs::read_to_string(provider_model_cache_path(cache_dir, provider)) else {
        return Vec::new();
    };
    let Ok(cache) = serde_json::from_str::<ProviderModelCache>(&content) else {
        return Vec::new();
    };
    provider_model_names(cache.models)
}

fn provider_model_names(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    for model in values {
        append_provider_model_name(&model, &mut models, &mut seen);
    }
    models
}

fn provider_model_cache_path(cache_dir: &Path, provider: &str) -> PathBuf {
    cache_dir.join(format!("{provider}.models.json"))
}

fn provider_api_key(config: &ProviderConfig, provider: &str) -> Option<String> {
    resolve_api_key_from_env_names(
        &provider_api_key_env_names(config),
        &provider_keychain_service(provider),
        "default",
    )
    .ok()
    .flatten()
}

fn provider_api_key_env_names(config: &ProviderConfig) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = config.api_key_env.as_deref()
        && is_env_name(name)
    {
        names.push(name.to_owned());
    }
    if let Some(host) = provider_host(&config.base_url) {
        append_env_name_for_host(&host, true, &mut names);
        append_env_name_for_host(&host, false, &mut names);
    }
    names
}

fn append_env_name_for_host(host: &str, drop_api_prefix: bool, names: &mut Vec<String>) {
    let labels = host
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let labels = if drop_api_prefix && labels.first() == Some(&"api") {
        labels.get(1..).unwrap_or_default()
    } else {
        labels.as_slice()
    };
    if labels.is_empty() {
        return;
    }
    let name = labels
        .iter()
        .map(|part| part.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("_")
        + "_API_KEY";
    if is_env_name(&name) && !names.contains(&name) {
        names.push(name);
    }
}

fn is_env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

fn provider_keychain_service(provider: &str) -> String {
    format!("cortexfs:{provider}")
}

#[derive(Deserialize)]
struct ProviderModelList {
    data: Vec<ProviderModelListItem>,
}

#[derive(Deserialize)]
struct ProviderModelListItem {
    id: String,
}

fn fetch_provider_models(base_url: &str, api_key: &str) -> Result<Vec<String>, FuseV1Error> {
    let output = run_curl_json(&provider_models_url(base_url), api_key)?;
    let list =
        serde_json::from_slice::<ProviderModelList>(&output).map_err(|_error| FuseV1Error::Io)?;
    Ok(list.data.into_iter().map(|model| model.id).collect())
}

fn run_curl_json(url: &str, api_key: &str) -> Result<Vec<u8>, FuseV1Error> {
    let mut child = Command::new("curl")
        .arg("--config")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_error| FuseV1Error::Io)?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err(FuseV1Error::Io);
    };
    let config = format!(
        "fail\nsilent\nshow-error\nmax-time = 20\nurl = {}\nheader = {}\n",
        curl_config_quote(url),
        curl_config_quote(&format!("Authorization: Bearer {api_key}"))
    );
    stdin
        .write_all(config.as_bytes())
        .map_err(|_error| FuseV1Error::Io)?;
    drop(stdin);
    let output = child.wait_with_output().map_err(|_error| FuseV1Error::Io)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(FuseV1Error::Io)
    }
}

fn curl_config_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

fn provider_models_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.rsplit('/').next() == Some("v1") {
        format!("{base}/models")
    } else {
        format!("{base}/v1/models")
    }
}
