use super::catalog::{PROVIDER_PRESETS, PresetTemplate, render_chat};
use crate::*;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProviderPreset {
    pub(crate) name: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) file: &'static str,
    pub(crate) auth: &'static str,
    pub(crate) template: PresetTemplate,
}

impl ProviderPreset {
    pub(crate) fn config(self) -> Cow<'static, str> {
        match self.template {
            PresetTemplate::Literal(config) => Cow::Borrowed(config),
            PresetTemplate::Chat { name, base, model } => {
                Cow::Owned(render_chat(name, base, model))
            }
        }
    }
}

pub(crate) fn provider_preset_list() -> Result<(), CliError> {
    for preset in PROVIDER_PRESETS {
        print_line(&format!(
            "{}\t{}\t{}",
            preset.name, preset.auth, preset.file
        ))?;
    }
    Ok(())
}

pub(crate) fn provider_preset_show(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    print_line(preset.config().trim_end())
}

pub(crate) fn provider_preset_install(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    write_preset_file(preset.file, &preset.config())
}

pub(crate) fn provider_preset_install_compatible(
    name: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
) -> Result<(), CliError> {
    let name = name.ok_or_else(|| CliError::usage("compatible requires --name"))?;
    let base_url = base_url.ok_or_else(|| CliError::usage("compatible requires --base-url"))?;
    if !is_provider_name(name) || is_model_alias(name) || matches!(name, "debug" | "route") {
        return Err(CliError::usage("invalid compatible provider name"));
    }
    if cortexfs::provider_host_from_base_url(base_url).is_none() {
        return Err(CliError::usage("invalid compatible --base-url"));
    }
    if let Some(model) = model {
        if !is_object_name(model) {
            return Err(CliError::usage("invalid compatible --model"));
        }
    }
    write_preset_file(&format!("{name}.json"), &render_chat(name, base_url, model))
}

fn write_preset_file(file: &str, config: &str) -> Result<(), CliError> {
    create_provider_config_dir(Path::new(PROVIDER_CONFIG_DIR))?;
    let path = PathBuf::from(PROVIDER_CONFIG_DIR).join(file);
    atomic_write_provider_config(&path, config)?;
    print_line(&format!("installed {}", path.display()))
}

pub(crate) fn provider_preset(name: &str) -> Result<ProviderPreset, CliError> {
    PROVIDER_PRESETS
        .iter()
        .copied()
        .find(|preset| preset.name == name || preset.aliases.contains(&name))
        .ok_or_else(|| CliError::usage(format!("unknown provider preset: {name}")))
}
