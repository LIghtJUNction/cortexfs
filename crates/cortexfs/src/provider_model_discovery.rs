const MAX_PROVIDER_MODEL_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_PROVIDER_MODEL_CACHE_BYTES: u64 = 1024 * 1024;
const MAX_PROVIDER_MODEL_COUNT: usize = 256;
const CURL_BIN: &str = "/usr/bin/curl";

pub fn refresh_provider_model_cache(config_dir: &Path, cache_dir: &Path) -> Result<(), FuseV1Error> {
    create_provider_model_cache_dir(cache_dir)?;
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

fn create_provider_model_cache_dir(path: &Path) -> Result<(), FuseV1Error> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_provider_model_cache_dir(path)
        } else {
            Err(FuseV1Error::Io)
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(FuseV1Error::Io);
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(_error) => return Err(FuseV1Error::Io),
        }
    }
    let existing_parent = missing
        .last()
        .and_then(|path| path.parent())
        .ok_or(FuseV1Error::Io)?;
    let mut parent_dir = open_provider_model_cache_dir(existing_parent)?;
    for directory in missing.iter().rev() {
        let name = provider_model_cache_file_name(directory)?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        )
        .map_err(|_error| FuseV1Error::Io)?;
        parent_dir.sync_all().map_err(|_error| FuseV1Error::Io)?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|_error| FuseV1Error::Io)?;
        parent_dir = fs::File::from(child);
        parent_dir.sync_all().map_err(|_error| FuseV1Error::Io)?;
    }
    Ok(())
}

fn sync_provider_model_cache_dir(path: &Path) -> Result<(), FuseV1Error> {
    let directory = open_provider_model_cache_dir(path)?;
    directory.sync_all().map_err(|_error| FuseV1Error::Io)
}

fn open_provider_model_cache_dir(path: &Path) -> Result<fs::File, FuseV1Error> {
    let mut directory = if path.is_absolute() {
        open_provider_model_cache_dir_leaf(Path::new("/"))?
    } else {
        open_provider_model_cache_dir_leaf(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or(FuseV1Error::Io)?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map_err(|_error| FuseV1Error::Io)?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(FuseV1Error::Io);
            }
        }
    }
    Ok(directory)
}

fn open_provider_model_cache_dir_leaf(path: &Path) -> Result<fs::File, FuseV1Error> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_error| FuseV1Error::Io)?;
    if !directory
        .metadata()
        .map_err(|_error| FuseV1Error::Io)?
        .is_dir()
    {
        return Err(FuseV1Error::Io);
    }
    Ok(directory)
}

fn provider_model_cache_file_name(path: &Path) -> Result<&str, FuseV1Error> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or(FuseV1Error::Io)
}

fn provider_cached_models(cache_dir: &Path, provider: &str) -> Vec<String> {
    let path = provider_model_cache_path(cache_dir, provider);
    let Ok(content) = read_fuse_v1_small_text_file(&path, MAX_PROVIDER_MODEL_CACHE_BYTES) else {
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
    let mut child = curl_command()
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
        curl_config_quote(url)?,
        curl_config_quote(&format!("Authorization: Bearer {api_key}"))?
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

fn curl_command() -> Command {
    let mut command = Command::new(CURL_BIN);
    command.env_clear().arg("-q").arg("--config").arg("-");
    command
}

fn terminate_child(child: &mut Child) {
    match child.kill() {
        Ok(()) | Err(_) => {}
    }
    match child.wait() {
        Ok(_) | Err(_) => {}
    }
}

fn curl_config_quote(value: &str) -> Result<String, FuseV1Error> {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if matches!(character, '\0' | '\n' | '\r') {
            return Err(FuseV1Error::InvalidContent);
        }
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    Ok(quoted)
}

fn provider_models_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.rsplit('/').next() == Some("v1") {
        format!("{base}/models")
    } else {
        format!("{base}/v1/models")
    }
}

#[cfg(test)]
mod provider_model_discovery_tests {
    use super::{
        CURL_BIN, FuseV1Error, curl_command, curl_config_quote, refresh_provider_model_cache,
    };
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn provider_model_discovery_uses_absolute_curl_path() {
        let command = curl_command();
        assert_eq!(command.get_program(), CURL_BIN);
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["-q", "--config", "-"]
        );
        assert!(command.get_envs().next().is_none());
    }

    #[test]
    fn provider_model_discovery_curl_quote_rejects_line_breaks() {
        assert!(curl_config_quote("https://api.openai.com/v1/models").is_ok());
        assert!(curl_config_quote("https://api.openai.com/v1\noutput = /tmp/leak").is_err());
        assert!(curl_config_quote("Authorization: Bearer bad\rheader = injected").is_err());
        assert!(curl_config_quote("abc\0def").is_err());
    }

    #[test]
    fn provider_model_discovery_rejects_symlink_cache_dir() {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-model-cache-symlink-{}",
            std::process::id()
        ));
        let outside = root.join("outside");
        let cache = root.join("cache");
        let config = root.join("missing-providers.d");
        let _ignored = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&outside).is_ok());
        assert!(symlink(&outside, &cache).is_ok());

        assert_eq!(
            refresh_provider_model_cache(&config, &cache),
            Err(FuseV1Error::Io)
        );
        assert!(cache
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink()));
        assert!(!outside.join("local.models.json").exists());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn provider_model_discovery_rejects_symlink_cache_parent_dir() {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-model-cache-parent-symlink-{}",
            std::process::id()
        ));
        let outside = root.join("outside");
        let link = root.join("link");
        let cache = link.join("existing").join("cache");
        let config = root.join("missing-providers.d");
        let _ignored = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(outside.join("existing")).is_ok());
        assert!(symlink(&outside, &link).is_ok());

        assert_eq!(
            refresh_provider_model_cache(&config, &cache),
            Err(FuseV1Error::Io)
        );
        assert!(!outside.join("existing").join("cache").exists());

        let _ignored = fs::remove_dir_all(&root);
    }
}
