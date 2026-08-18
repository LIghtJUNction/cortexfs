use std::path::{Path, PathBuf};

pub const SYSTEM_STORAGE_DIR: &str = "/var/lib/cortexfs/storage";
pub const SYSTEM_STORAGE_CURRENT: &str = "/var/lib/cortexfs/storage/current";
pub const SYSTEM_PROVIDER_CONFIG_DIR: &str = "/etc/cortexfs/providers.d";
pub const SYSTEM_PROVIDER_SECRET_DIR: &str = "/var/lib/cortexfs/secrets/provider";
pub const SYSTEM_PROVIDER_MODEL_CACHE_DIR: &str = "/var/lib/cortexfs/provider-models";
pub const SYSTEM_CHANNEL_CONFIG_DIR: &str = "/etc/cortexfs/channels";
pub const SYSTEM_AGENT_PROMPT_PATH: &str = "/etc/cortexfs/AGENTS.md";

#[must_use]
pub fn storage_root_path() -> PathBuf {
    PathBuf::from(SYSTEM_STORAGE_DIR)
}

#[must_use]
pub fn storage_generations_path(storage: &Path) -> PathBuf {
    storage.join("generations")
}

#[must_use]
pub fn storage_current_path() -> PathBuf {
    PathBuf::from(SYSTEM_STORAGE_CURRENT)
}

#[must_use]
pub fn storage_current_link_path(storage: &Path) -> PathBuf {
    storage.join("current")
}

#[must_use]
pub fn storage_update_lock_path(storage: &Path) -> PathBuf {
    storage.join(".update.lock")
}

#[must_use]
pub fn storage_generation_path(storage: &Path, generation: &str) -> PathBuf {
    storage.join("generations").join(generation)
}

#[must_use]
pub fn channel_config_path(channel: &str) -> PathBuf {
    Path::new(SYSTEM_CHANNEL_CONFIG_DIR).join(format!("{channel}.toml"))
}

#[must_use]
pub fn provider_config_path(provider: &str) -> PathBuf {
    Path::new(SYSTEM_PROVIDER_CONFIG_DIR).join(format!("{provider}.json"))
}

#[must_use]
pub fn provider_secret_path(provider: &str) -> PathBuf {
    Path::new(SYSTEM_PROVIDER_SECRET_DIR).join(provider)
}

#[must_use]
pub fn provider_secret_root_path() -> PathBuf {
    Path::new(SYSTEM_PROVIDER_SECRET_DIR).parent().map_or_else(
        || PathBuf::from(SYSTEM_PROVIDER_SECRET_DIR),
        Path::to_path_buf,
    )
}

#[must_use]
pub fn provider_model_cache_path() -> PathBuf {
    PathBuf::from(SYSTEM_PROVIDER_MODEL_CACHE_DIR)
}
