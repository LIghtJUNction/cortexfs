use crate::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProviderPreset {
    pub(crate) name: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) file: &'static str,
    pub(crate) config: &'static str,
}

const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "openai",
        aliases: &[],
        file: "api.openai.com.json",
        config: r#"{
            "base_url": "https://api.openai.com/v1",
            "default_model": "gpt-5.6",
            "models": ["gpt-5.6"],
            "enabled": true,
            "formats": ["openai.chat", "openai.responses"]
        } "#,
    },
    ProviderPreset {
        name: "codex",
        aliases: &["ccodex"],
        file: "chatgpt.com.json",
        config: r#"{
            "name": "codex",
            "base_url": "https://chatgpt.com/backend-api/codex",
            "default_model": "gpt-5.6",
            "models": ["gpt-5.6"],
            "enabled": true,
            "formats": ["openai.responses"],
            "auth": [{"type": "oauth", "flow": "authorization_code", "slot": "subscription"}],
            "oauth": {
                "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
                "auth_url": "https://auth.openai.com/oauth/authorize",
                "token_url": "https://auth.openai.com/oauth/token",
                "redirect_uri": "http://localhost:1455/auth/callback",
                "scopes": ["openid", "profile", "email", "offline_access", "api.connectors.read", "api.connectors.invoke"]
            }
        } "#,
    },
    ProviderPreset {
        name: "anthropic",
        aliases: &["claude"],
        file: "api.anthropic.com.json",
        config: r#"{
            "base_url": "https://api.anthropic.com/v1",
            "enabled": true,
            "formats": ["anthropic.messages"]
        } "#,
    },
    ProviderPreset {
        name: "google",
        aliases: &["gemini"],
        file: "generativelanguage.googleapis.com.json",
        config: r#"{
            "base_url": "https://generativelanguage.googleapis.com/v1beta/openai/",
            "enabled": true,
            "formats": ["openai.chat"]
        } "#,
    },
];

pub(crate) fn provider_preset_list() -> Result<(), CliError> {
    for preset in PROVIDER_PRESETS {
        print_line(preset.name)?;
    }
    Ok(())
}

pub(crate) fn provider_preset_show(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    print_line(preset.config.trim_end())
}

pub(crate) fn provider_preset_install(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    create_provider_config_dir(Path::new(PROVIDER_CONFIG_DIR))?;
    let path = PathBuf::from(PROVIDER_CONFIG_DIR).join(preset.file);
    atomic_write_provider_config(&path, preset.config)?;
    print_line(&format!("installed {}", path.display()))
}

pub(crate) fn provider_preset(name: &str) -> Result<ProviderPreset, CliError> {
    PROVIDER_PRESETS
        .iter()
        .copied()
        .find(|preset| preset.name == name || preset.aliases.contains(&name))
        .ok_or_else(|| CliError::usage(format!("unknown provider preset: {name}")))
}
