#[cfg(test)]
use std::collections::BTreeMap;
use std::fs;
#[cfg(not(test))]
use std::io::Write as _;
use std::path::PathBuf;
#[cfg(not(test))]
use std::process::{Command, Stdio};
use std::str::FromStr;
#[cfg(test)]
use std::sync::{LazyLock, Mutex};

use crate::providers::newline_list;
use cortex_core::{ApiFormat, ModelId, ProviderId};

#[cfg(test)]
static TEST_SECRETS: LazyLock<Mutex<BTreeMap<String, String>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RegistryProvider {
    pub id: String,
    pub family: String,
    pub name: String,
    pub formats: Vec<String>,
    pub base_url: String,
    pub default_model: String,
    pub priority: u32,
    pub enabled: bool,
    pub secret_status: String,
    pub secret_ref: String,
}

impl RegistryProvider {
    pub fn supports_format(&self, format: &str) -> bool {
        self.formats.iter().any(|candidate| candidate == format)
    }

    pub fn formats_text(&self) -> String {
        newline_list(self.formats.iter())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderRegistry {
    dir: PathBuf,
}

impl ProviderRegistry {
    #[cfg(test)]
    pub fn from_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn from_env() -> Option<Self> {
        let dir = std::env::var("CORTEXFS_PROVIDER_CONFIG_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .or_else(default_config_dir)?;
        Some(Self { dir })
    }

    pub fn load(&self) -> Vec<RegistryProvider> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|content| parse_provider_config(&content).ok())
            .collect()
    }

    pub fn upsert(&self, content: &str) -> Result<RegistryProvider, String> {
        let provider = parse_provider_config(content)?;
        fs::create_dir_all(&self.dir).map_err(|error| error.to_string())?;
        let path = self.dir.join(format!("{}.json", provider.id));
        fs::write(path, canonical_provider_json(&provider)).map_err(|error| error.to_string())?;
        Ok(provider)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SecretStore;

impl SecretStore {
    pub fn store_provider_key(provider: &str, value: &str) -> Result<String, String> {
        store_provider_key(provider, value)
    }

    pub fn lookup_provider_key(provider: &str) -> Result<String, String> {
        lookup_provider_key(provider)
    }

    #[cfg(test)]
    pub fn clear_test_secrets() {
        if let Ok(mut secrets) = TEST_SECRETS.lock() {
            secrets.clear();
        }
    }
}

#[cfg(not(test))]
fn store_provider_key(provider: &str, value: &str) -> Result<String, String> {
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label",
            &format!("CortexFS provider {provider} API key"),
            "application",
            "cortexfs",
            "provider",
            provider,
            "kind",
            "api-key",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let Some(stdin) = child.stdin.as_mut() else {
        return Err("failed to open secret-tool stdin".to_owned());
    };
    stdin
        .write_all(value.as_bytes())
        .map_err(|error| error.to_string())?;
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if error.is_empty() {
            format!("secret-tool exited with {}", output.status)
        } else {
            error
        });
    }
    Ok(format!("secret-service:provider:{provider}:api-key"))
}

#[cfg(test)]
fn store_provider_key(provider: &str, value: &str) -> Result<String, String> {
    TEST_SECRETS
        .lock()
        .map_err(|error| error.to_string())?
        .insert(provider.to_owned(), value.to_owned());
    Ok(format!("secret-service:provider:{provider}:api-key"))
}

#[cfg(not(test))]
fn lookup_provider_key(provider: &str) -> Result<String, String> {
    let output = Command::new("secret-tool")
        .args([
            "lookup",
            "application",
            "cortexfs",
            "provider",
            provider,
            "kind",
            "api-key",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("secret-tool exited with {}", output.status));
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        return Err("missing provider API key in Secret Service".to_owned());
    }
    Ok(value)
}

#[cfg(test)]
fn lookup_provider_key(provider: &str) -> Result<String, String> {
    TEST_SECRETS
        .lock()
        .map_err(|error| error.to_string())?
        .get(provider)
        .cloned()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing provider API key in Secret Service".to_owned())
}

fn default_config_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join(".config/cortexfs/providers.d"))
}

fn parse_provider_config(content: &str) -> Result<RegistryProvider, String> {
    let value =
        serde_json::from_str::<serde_json::Value>(content).map_err(|error| error.to_string())?;
    let id = string_field(&value, "id")?;
    ProviderId::new(id.clone()).map_err(|error| error.to_string())?;
    let family = string_field(&value, "family").unwrap_or_else(|_| "openai-compatible".to_owned());
    let name = string_field(&value, "name").unwrap_or_else(|_| id.clone());
    let formats = string_array_field(&value, "formats")
        .unwrap_or_else(|_| vec!["openai.chat".to_owned(), "openai.responses".to_owned()]);
    for format in &formats {
        ApiFormat::from_str(format).map_err(|error| error.to_string())?;
    }
    let base_url = string_field(&value, "base_url")?;
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err("base_url must start with http:// or https://".to_owned());
    }
    let default_model =
        string_field(&value, "default_model").unwrap_or_else(|_| "gpt-4o-mini".to_owned());
    ModelId::new(default_model.clone()).map_err(|error| error.to_string())?;
    let priority = value
        .get("priority")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(80);
    let enabled = value
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    Ok(RegistryProvider {
        id,
        family,
        name,
        formats,
        base_url,
        default_model,
        priority,
        enabled,
        secret_status: "missing".to_owned(),
        secret_ref: "none".to_owned(),
    })
}

fn canonical_provider_json(provider: &RegistryProvider) -> String {
    serde_json::json!({
        "id": provider.id,
        "family": provider.family,
        "name": provider.name,
        "formats": provider.formats,
        "base_url": provider.base_url,
        "default_model": provider.default_model,
        "priority": provider.priority,
        "enabled": provider.enabled,
    })
    .to_string()
}

fn string_field(value: &serde_json::Value, field: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("missing string field: {field}"))
}

fn string_array_field(value: &serde_json::Value, field: &str) -> Result<Vec<String>, String> {
    let values = value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| format!("missing array field: {field}"))?;
    let values = values
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(format!("empty array field: {field}"));
    }
    Ok(values)
}
