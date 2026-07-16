use crate::*;

/// One built-in provider preset exposed by the CLI.
///
/// A preset includes:
/// - the canonical provider name used for matching,
/// - optional aliases accepted on the CLI,
/// - the target JSON config filename,
/// - and inline JSON config text used when writing files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProviderPreset {
    /// Canonical preset key.
    pub(crate) name: &'static str,
    /// CLI-friendly aliases that map to this preset.
    pub(crate) aliases: &'static [&'static str],
    /// Filename to persist this preset as under the provider config directory.
    pub(crate) file: &'static str,
    /// Full JSON template for this provider preset.
    pub(crate) config: &'static str,
}

/// Static list of supported built-in provider presets.
const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "openai",
        aliases: &["codex", "ccodex"],
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

/// Print all built-in provider preset names to standard output.
pub(crate) fn provider_preset_list() -> Result<(), CliError> {
    for preset in PROVIDER_PRESETS {
        print_line(preset.name)?;
    }
    Ok(())
}

/// Print a resolved preset's raw JSON config.
///
/// This keeps output identical to the stored template aside from trailing-space trimming.
pub(crate) fn provider_preset_show(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    print_line(preset.config.trim_end())
}

/// Install a preset JSON file into the provider config directory.
///
/// The function resolves a preset by name/alias and writes the embedded JSON
/// template to the destination path derived from that preset's `file`.
pub(crate) fn provider_preset_install(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    create_provider_config_dir(Path::new(PROVIDER_CONFIG_DIR))?;
    let path = PathBuf::from(PROVIDER_CONFIG_DIR).join(preset.file);
    atomic_write_provider_config(&path, preset.config)?;
    print_line(&format!("installed {}", path.display()))
}

/// Resolve a preset name or alias to an in-memory preset entry.
///
/// Returns a usage error when no built-in preset matches.
pub(crate) fn provider_preset(name: &str) -> Result<ProviderPreset, CliError> {
    PROVIDER_PRESETS
        .iter()
        .copied()
        .find(|preset| preset.name == name || preset.aliases.contains(&name))
        .ok_or_else(|| CliError::usage(format!("unknown provider preset: {name}")))
}
