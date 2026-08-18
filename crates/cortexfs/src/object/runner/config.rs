use super::*;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn provider_config(provider: &str) -> Option<RunnerProviderConfig> {
    let config_dir = env::var_os("CTX_PROVIDER_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(RUNNER_PROVIDER_CONFIG_DIR), PathBuf::from);
    provider_config_from_dir(&config_dir, provider)
}
pub(crate) fn provider_config_from_model_control(
    ctx_root: &Path,
    provider: &str,
    model: &str,
) -> Option<RunnerProviderConfig> {
    let control = cortexfs_paths::model_control_path(ctx_root, provider, model);
    let default =
        read_small_plain_text_file(&control.join("default"), MAX_RUNNER_CONTROL_BYTES, "runner")
            .ok()?;
    let base_url = model_default_base_url(&default)?;
    let driver =
        read_small_plain_text_file(&control.join("driver"), MAX_RUNNER_CONTROL_BYTES, "runner")
            .unwrap_or_default();
    Some(RunnerProviderConfig {
        name: Some(provider.to_owned()),
        base_url,
        auth: Vec::new(),
        oauth: None,
        formats: model_driver_formats(&driver),
    })
}
pub(crate) fn model_driver_formats(content: &str) -> Vec<String> {
    let mut formats = Vec::new();
    if content.contains("openai.chat") || content.contains("openai-chat") {
        formats.push("openai.chat".to_owned());
    }
    if content.contains("openai.responses") || content.contains("openai-responses") {
        formats.push("openai.responses".to_owned());
    }
    if formats.is_empty() {
        formats.push("openai.chat".to_owned());
    }
    formats
}
pub(crate) fn provider_config_from_dir(
    config_dir: &Path,
    provider: &str,
) -> Option<RunnerProviderConfig> {
    let directory = open_plain_directory(config_dir).ok()?;
    let entries = fs::read_dir(proc_fd_path(&directory)).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().into_string().ok()?;
        if Path::new(&name)
            .extension()
            .and_then(|value| value.to_str())
            != Some("json")
        {
            continue;
        }
        let content = cortexfs::support::plain::read_small_text_file_at(
            &directory,
            &name,
            MAX_RUNNER_PROVIDER_CONFIG_BYTES,
            "provider config file is invalid",
        )
        .ok()?;
        let config = serde_json::from_str::<RunnerProviderConfig>(&content).ok()?;
        if cortexfs::provider_name_from_config(&config.base_url, config.name.as_deref()).as_deref()
            != Ok(provider)
        {
            continue;
        }
        return Some(config);
    }
    None
}
#[cfg(test)]
mod runner_provider_config_tests {
    use super::*;
    use std::io;
    #[test]
    fn provider_config_can_fall_back_to_model_control_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let control = root.path().join("model/api.test/gpt-5.6-terra.d");
        fs::create_dir_all(&control)?;
        fs::write(control.join("default"), "base_url=https://api.test/v1\n")?;
        fs::write(
            control.join("driver"),
            "default=openai-chat\nagent=openai-responses,openai-chat\n",
        )?;
        let config = provider_config_from_model_control(root.path(), "api.test", "gpt-5.6-terra")
            .ok_or_else(|| io::Error::other("missing fallback provider config"))?;
        assert_eq!(config.name.as_deref(), Some("api.test"));
        assert_eq!(config.base_url, "https://api.test/v1");
        assert_eq!(config.formats, ["openai.chat", "openai.responses"]);
        Ok(())
    }
}
