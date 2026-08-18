use crate::*;

use crate::provider::auth::AuthMethod;
use crate::provider::auth::{
    AuthProvider, AuthProviderError, AuthResponse, AuthTransport, Credential, configured_registry,
};
use crate::provider::name::is_reserved_provider_name;
use crate::support::command::CURL;
use crate::support::plain::{create_plain_dir, open_plain_directory, read_small_text_file};
use crate::support::receipt::{
    EmptyDirReceipt, EntryKind, EntryReceipt, park_entry, receipt_at, remove_parked_entry,
};

const MAX_PROVIDER_MODEL_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_PROVIDER_MODEL_CACHE_BYTES: u64 = 1024 * 1024;
const MAX_PROVIDER_MODEL_COUNT: usize = 256;
pub fn refresh_provider_model_cache(config_dir: &Path, cache_dir: &Path) -> Result<(), FuseError> {
    create_plain_dir(cache_dir).map_err(|_error| FuseError::Io)?;
    let snapshot = ProviderSnapshot::load(config_dir, cache_dir).map_err(|_error| FuseError::Io)?;
    let active = snapshot.active();
    let mut mutated = prune_inactive_provider_model_caches(cache_dir, active)?;
    for entry in snapshot.configs() {
        let provider = &entry.0;
        let config = &entry.1;
        if !config.enabled {
            continue;
        }
        let Some(registry) = configured_registry(
            provider,
            &config.base_url,
            config.auth_methods(),
            config.oauth.clone(),
        ) else {
            continue;
        };
        let Some(adapter) = registry.get(provider) else {
            continue;
        };
        let Some(credential) = provider_credential(config, provider, adapter) else {
            continue;
        };
        let mut transport = ModelDiscoveryTransport;
        let Ok(models) = adapter.models_with(Some(&credential), &mut transport) else {
            continue;
        };
        let models = provider_model_names(models);
        if models.is_empty() {
            continue;
        }
        let content = serde_json::json!({ "models": models }).to_string() + "\n";
        atomic_replace_text(&provider_model_cache_path(cache_dir, provider), &content)
            .map_err(|_error| FuseError::Io)?;
        mutated = true;
    }
    if mutated {
        support::plain::sync_plain_dir(cache_dir).map_err(|_error| FuseError::Io)?;
    }
    Ok(())
}

struct ModelDiscoveryTransport;

impl AuthTransport for ModelDiscoveryTransport {
    fn post(
        &mut self,
        _url: &str,
        _content_type: &str,
        _body: &str,
    ) -> Result<AuthResponse, AuthProviderError> {
        Err(AuthProviderError::UnsupportedMethod)
    }

    fn get(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<AuthResponse, AuthProviderError> {
        let headers = headers
            .iter()
            .map(|&(name, value)| (name.to_owned(), value.to_owned()))
            .collect::<Vec<_>>();
        run_curl_json(url, &headers)
            .map(|body| AuthResponse { status: 200, body })
            .map_err(|error| match error {
                FuseError::TooLarge => AuthProviderError::InvalidResponse,
                FuseError::InvalidContent => AuthProviderError::InvalidConfig,
                _ => AuthProviderError::Unavailable,
            })
    }
}

fn prune_inactive_provider_model_caches(
    cache_dir: &Path,
    active: &HashSet<String>,
) -> Result<bool, FuseError> {
    let directory = open_plain_directory(cache_dir).map_err(|_error| FuseError::Io)?;
    let entries =
        fs::read_dir(support::plain::proc_fd_path(&directory)).map_err(|_error| FuseError::Io)?;
    let mut mutated = false;
    for entry in entries {
        let name = entry.map_err(|_error| FuseError::Io)?.file_name();
        let name = name.to_str().ok_or(FuseError::InvalidPath)?;
        let Some(provider) = name.strip_suffix(".models.json") else {
            continue;
        };
        if !is_object_name(provider)
            || is_reserved_provider_name(provider)
            || active.contains(provider)
        {
            continue;
        }
        let stat = match nix::sys::stat::fstatat(
            &directory,
            name,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
            Ok(stat) => stat,
            Err(nix::errno::Errno::ENOENT) => continue,
            Err(_) => return Err(FuseError::Io),
        };
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            continue;
        }
        if let Some(receipt) =
            receipt_at(&directory, name, EntryKind::File).map_err(|_error| FuseError::Io)?
        {
            mutated |= isolate_inactive_cache(cache_dir, &directory, name, receipt)?;
        }
    }
    Ok(mutated)
}

fn isolate_inactive_cache(
    cache_dir: &Path,
    parent: &fs::File,
    name: &str,
    receipt: EntryReceipt,
) -> Result<bool, FuseError> {
    let owner = (
        nix::unistd::getuid().as_raw(),
        nix::unistd::getgid().as_raw(),
    );
    let stage = EmptyDirReceipt::create(cache_dir, ".cortexfs-cache", owner.0, owner.1, 0o700)
        .map_err(|_error| FuseError::Io)?;
    let stage_dir = open_plain_directory(stage.path()).map_err(|_error| FuseError::Io)?;
    park_entry(parent, name, &stage_dir, "entry", receipt, EntryKind::File)
        .map_err(|_error| FuseError::Io)?;
    remove_parked_entry(&stage_dir, "entry", receipt, EntryKind::File)
        .map_err(|_error| FuseError::Io)?;
    stage.cleanup().map_err(|_error| FuseError::Io)?;
    Ok(true)
}
pub(crate) fn provider_cached_models(cache_dir: &Path, provider: &str) -> Vec<String> {
    let path = provider_model_cache_path(cache_dir, provider);
    let Ok(content) = read_small_text_file(&path, MAX_PROVIDER_MODEL_CACHE_BYTES) else {
        return Vec::new();
    };
    let Ok(cache) = serde_json::from_str::<ProviderModelCache>(&content) else {
        return Vec::new();
    };
    provider_model_names(cache.models)
}

pub(crate) fn provider_model_names(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    for model in values {
        let model = model.trim();
        if is_object_name(model) && seen.insert(model.to_owned()) {
            models.push(model.to_owned());
        }
        if models.len() >= MAX_PROVIDER_MODEL_COUNT {
            break;
        }
    }
    models
}

pub(crate) fn provider_model_cache_path(cache_dir: &Path, provider: &str) -> PathBuf {
    cache_dir.join(format!("{provider}.models.json"))
}

fn provider_credential(
    config: &ProviderConfig,
    provider: &str,
    adapter: &dyn AuthProvider,
) -> Option<Credential> {
    let methods = config.auth_methods();
    let api_key = methods
        .iter()
        .find(|method| method.method == AuthMethod::ApiKey)
        .and_then(|method| {
            read_provider_system_secret(provider, &method.slot)
                .ok()
                .flatten()
        });
    if let Some(key) = api_key {
        return Some(Credential::ApiKey {
            provider: provider.to_owned(),
            key,
            slot: methods
                .iter()
                .find(|method| method.method == AuthMethod::ApiKey)
                .map(|method| method.slot.clone()),
        });
    }
    if !methods
        .iter()
        .any(|method| method.method == AuthMethod::OAuth)
    {
        return None;
    }
    let oauth = config.oauth.as_ref()?;
    resolve_oauth_credential_with(provider, oauth, |request| {
        refresh_oauth_result(provider, request, adapter)
    })
    .ok()
    .flatten()
    .map(|(access_token, _account)| Credential::OAuth {
        provider: provider.to_owned(),
        access_token,
        refresh_token: None,
        expires_at: None,
        scopes: Vec::new(),
    })
}

pub(crate) fn run_curl_json(url: &str, headers: &[(String, String)]) -> Result<Vec<u8>, FuseError> {
    let mut child = curl_command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_error| FuseError::Io)?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err(FuseError::Io);
    };
    let mut config = format!(
        "fail\nsilent\nshow-error\nmax-time = 20\nurl = {}\n",
        curl_config_quote(url)?
    );
    for header in headers {
        config.push_str("header = ");
        config.push_str(&curl_config_quote(&format!("{}: {}", header.0, header.1))?);
        config.push('\n');
    }
    stdin
        .write_all(config.as_bytes())
        .map_err(|_error| FuseError::Io)?;
    drop(stdin);
    let Some(stdout) = child.stdout.take() else {
        terminate_child(&mut child);
        return Err(FuseError::Io);
    };
    let mut limited = stdout.take(MAX_PROVIDER_MODEL_RESPONSE_BYTES + 1);
    let mut output = Vec::new();
    limited
        .read_to_end(&mut output)
        .map_err(|_error| FuseError::Io)?;
    let output_len = u64::try_from(output.len()).map_err(|_error| FuseError::TooLarge)?;
    if output_len > MAX_PROVIDER_MODEL_RESPONSE_BYTES {
        terminate_child(&mut child);
        return Err(FuseError::TooLarge);
    }
    let status = child.wait().map_err(|_error| FuseError::Io)?;
    if status.success() {
        Ok(output)
    } else {
        Err(FuseError::Io)
    }
}

pub(crate) fn curl_command() -> Command {
    let mut command = Command::new(CURL);
    command.env_clear().arg("-q").arg("--config").arg("-");
    command
}

pub(crate) fn terminate_child(child: &mut Child) {
    match child.kill() {
        Ok(()) | Err(_) => {}
    }
    match child.wait() {
        Ok(_) | Err(_) => {}
    }
}

pub(crate) fn curl_config_quote(value: &str) -> Result<String, FuseError> {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if character.is_ascii_control() {
            return Err(FuseError::InvalidContent);
        }
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    Ok(quoted)
}

#[cfg(test)]
mod provider_model_discovery_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn provider_model_discovery_uses_hardened_curl_config() {
        let command = curl_command();
        assert_eq!(command.get_program(), CURL);
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["-q", "--config", "-"]
        );
        assert!(command.get_envs().next().is_none());
        for value in [
            "https://api.openai.com/v1\noutput = /tmp/leak",
            "Authorization: Bearer bad\rheader = injected",
            "Authorization: Bearer \u{1b}]52;c;payload",
            "abc\0def",
        ] {
            assert!(curl_config_quote(value).is_err());
        }
    }

    #[test]
    fn provider_model_discovery_rejects_symlink_cache_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let outside = root.path().join("outside");
        let link = root.path().join("link");
        fs::DirBuilder::new().create(&outside)?;
        symlink(&outside, &link)?;
        for cache in [link.clone(), link.join("cache")] {
            assert_eq!(
                refresh_provider_model_cache(&root.path().join("missing"), &cache),
                Err(FuseError::Io)
            );
        }
        assert!(!outside.join("cache").exists());
        Ok(())
    }

    #[test]
    fn inactive_cache_prune_preserves_nonregular_and_concurrent_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let cache = root.path().join("cache");
        fs::DirBuilder::new().create(&cache)?;
        let old = cache.join("gone.models.json");
        fs::write(&old, "old")?;
        fs::DirBuilder::new().create(cache.join("dir.models.json"))?;
        symlink(root.path(), cache.join("link.models.json"))?;
        let previous = support::receipt::set_park_hook(Some(Box::new(|directory, name| {
            let path = support::plain::proc_fd_path(directory).join(name);
            fs::write(path, "new")?;
            Ok(())
        })));
        assert_eq!(
            refresh_provider_model_cache(&root.path().join("missing"), &cache),
            Err(FuseError::Io)
        );
        let _previous = support::receipt::set_park_hook(previous);
        assert_eq!(fs::read_to_string(&old)?, "new");
        assert!(cache.join("dir.models.json").is_dir());
        assert!(cache.join("link.models.json").symlink_metadata().is_ok());
        assert_eq!(
            refresh_provider_model_cache(&root.path().join("missing"), &cache),
            Ok(())
        );
        assert!(!old.exists());
        Ok(())
    }
}
