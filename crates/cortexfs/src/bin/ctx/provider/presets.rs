#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProviderPreset {
    name: &'static str,
    aliases: &'static [&'static str],
    file: &'static str,
    config: &'static str,
}

const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "openai",
        aliases: &["codex", "ccodex"],
        file: "api.openai.com.json",
        config: r#"{
  "base_url": "https://api.openai.com/v1",
  "default_model": "gpt-5.5",
  "models": ["gpt-5.4-mini"],
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
"#,
    },
    ProviderPreset {
        name: "anthropic",
        aliases: &["claude"],
        file: "api.anthropic.com.json",
        config: r#"{
  "base_url": "https://api.anthropic.com/v1",
  "enabled": true,
  "formats": ["anthropic.messages"]
}
"#,
    },
    ProviderPreset {
        name: "google",
        aliases: &["gemini"],
        file: "generativelanguage.googleapis.com.json",
        config: r#"{
  "base_url": "https://generativelanguage.googleapis.com/v1beta/openai/",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    },
];

fn provider_preset_list() -> Result<(), CliError> {
    for preset in PROVIDER_PRESETS {
        print_line(preset.name)?;
    }
    Ok(())
}

fn provider_preset_show(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    print_line(preset.config.trim_end())
}

fn provider_preset_install(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    create_provider_config_dir(Path::new(PROVIDER_CONFIG_DIR))?;
    let path = PathBuf::from(PROVIDER_CONFIG_DIR).join(preset.file);
    atomic_write_provider_config(&path, preset.config)?;
    print_line(&format!("installed {}", path.display()))
}

fn provider_preset(name: &str) -> Result<ProviderPreset, CliError> {
    PROVIDER_PRESETS
        .iter()
        .copied()
        .find(|preset| preset.name == name || preset.aliases.contains(&name))
        .ok_or_else(|| CliError::usage(format!("unknown provider preset: {name}")))
}
