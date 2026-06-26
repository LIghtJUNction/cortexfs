const MAX_PROVIDER_MODEL_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_PROVIDER_MODEL_CACHE_BYTES: u64 = 1024 * 1024;
const MAX_PROVIDER_MODEL_COUNT: usize = 256;

pub fn refresh_provider_model_cache(config_dir: &Path, cache_dir: &Path) -> Result<(), FuseV1Error> {
    fs::create_dir_all(cache_dir).map_err(|_error| FuseV1Error::Io)?;
    for config in read_provider_configs(config_dir)? {
        if !config.config.enabled {
            continue;
        }
        let Ok(provider) =
            provider_name_from_config(&config.config.base_url, config.config.name.as_deref())
        else {
            continue;
        };
        let Some(api_key) = provider_bearer_token(&config.config, &provider) else {
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
    let path = provider_model_cache_path(cache_dir, provider);
    let Ok(metadata) = fs::metadata(&path) else {
        return Vec::new();
    };
    if metadata.len() > MAX_PROVIDER_MODEL_CACHE_BYTES {
        return Vec::new();
    }
    let Ok(content) = fs::read_to_string(path) else {
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
        if models.len() >= MAX_PROVIDER_MODEL_COUNT {
            break;
        }
    }
    models
}

fn provider_model_cache_path(cache_dir: &Path, provider: &str) -> PathBuf {
    cache_dir.join(format!("{provider}.models.json"))
}

fn provider_bearer_token(config: &ProviderConfig, provider: &str) -> Option<String> {
    let api_key = read_provider_system_secret(provider, "default").ok().flatten();
    if api_key.is_some() {
        return api_key;
    }
    let oauth = config.oauth.as_ref()?;
    resolve_oauth_access_token(provider, oauth).ok().flatten()
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
    let Some(stdout) = child.stdout.take() else {
        terminate_child(&mut child);
        return Err(FuseV1Error::Io);
    };
    let mut limited = stdout.take(MAX_PROVIDER_MODEL_RESPONSE_BYTES + 1);
    let mut output = Vec::new();
    limited
        .read_to_end(&mut output)
        .map_err(|_error| FuseV1Error::Io)?;
    let output_len = u64::try_from(output.len()).map_err(|_error| FuseV1Error::TooLarge)?;
    if output_len > MAX_PROVIDER_MODEL_RESPONSE_BYTES {
        terminate_child(&mut child);
        return Err(FuseV1Error::TooLarge);
    }
    let status = child.wait().map_err(|_error| FuseV1Error::Io)?;
    if status.success() {
        Ok(output)
    } else {
        Err(FuseV1Error::Io)
    }
}

fn terminate_child(child: &mut Child) {
    match child.kill() {
        Ok(()) | Err(_) => {}
    }
    match child.wait() {
        Ok(_) | Err(_) => {}
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
