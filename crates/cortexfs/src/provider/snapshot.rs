#[cfg(test)]
use std::{cell::RefCell, thread_local};
use std::{collections::HashSet, fs, path::Path};

use super::config::{ProjectedProviderModel, ProviderConfig};
use super::name::provider_name_from_config;
use super::project::project_models;
use crate::{
    STABLE_MODEL_CAPABILITIES,
    support::plain::{open_plain_directory, proc_fd_path, read_small_text_file_at},
};

const MAX_CONFIG_BYTES: u64 = 64 * 1024;
#[cfg(test)]
pub type LoadHook = Box<dyn FnOnce()>;

#[cfg(test)]
thread_local! {
    static LOAD_HOOK: RefCell<Option<LoadHook>> = RefCell::new(None);
}

#[cfg(test)]
pub fn set_load_hook(hook: Option<LoadHook>) -> Option<LoadHook> {
    LOAD_HOOK.with(|slot| slot.replace(hook))
}

#[cfg(test)]
fn run_load_hook() {
    LOAD_HOOK.with(|slot| {
        if let Some(hook) = slot.take() {
            hook();
        }
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderError {
    Invalid,
    Io,
}

pub struct ProviderSnapshot {
    configs: Vec<(String, ProviderConfig)>,
    active: HashSet<String>,
    models: Vec<ProjectedProviderModel>,
}

impl ProviderSnapshot {
    pub(crate) fn load(config_dir: &Path, cache_dir: &Path) -> Result<Self, ProviderError> {
        let configs = read_configs(config_dir)?;
        #[cfg(test)]
        run_load_hook();
        let mut resolved = Vec::new();
        let mut active = HashSet::new();
        let mut models = Vec::new();
        for config in configs {
            let provider = provider_name_from_config(&config.base_url, config.name.as_deref())
                .map_err(|_error| ProviderError::Invalid)?;
            if config.enabled {
                active.insert(provider.clone());
                project_models(&provider, &config, cache_dir, &mut models);
            }
            resolved.push((provider, config));
        }
        models.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.model.cmp(&right.model))
        });
        Ok(Self {
            configs: resolved,
            active,
            models,
        })
    }

    pub(crate) fn configs(&self) -> &[(String, ProviderConfig)] {
        &self.configs
    }

    pub(crate) fn active(&self) -> &HashSet<String> {
        &self.active
    }

    pub(crate) fn models(&self) -> &[ProjectedProviderModel] {
        &self.models
    }
}

fn read_configs(config_dir: &Path) -> Result<Vec<ProviderConfig>, ProviderError> {
    let directory = match open_plain_directory(config_dir) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(ProviderError::Io),
    };
    let entries = fs::read_dir(proc_fd_path(&directory)).map_err(|_error| ProviderError::Io)?;
    let mut configs = Vec::new();
    for entry in entries {
        let name = entry.map_err(|_error| ProviderError::Io)?.file_name();
        let Some(name) = name.to_str() else {
            return Err(ProviderError::Invalid);
        };
        if Path::new(name).extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let content = read_small_text_file_at(&directory, name, MAX_CONFIG_BYTES, "invalid")
            .map_err(|_error| ProviderError::Invalid)?;
        let config = serde_json::from_str::<ProviderConfig>(&content)
            .map_err(|_error| ProviderError::Invalid)?;
        if !valid_model_metadata(&config) {
            return Err(ProviderError::Invalid);
        }
        configs.push(config);
    }
    Ok(configs)
}

fn valid_model_metadata(config: &ProviderConfig) -> bool {
    let declared = |model: &String| {
        config.default_model.as_ref() == Some(model) || config.models.contains(model)
    };
    config
        .model_limits
        .iter()
        .all(|(model, limit)| *limit > 0 && declared(model))
        && config.model_capabilities.iter().all(|(model, capabilities)| {
            let mut seen = HashSet::<&str>::new();
            declared(model)
                && capabilities.iter().all(|capability| {
                    STABLE_MODEL_CAPABILITIES.contains(&capability.as_str())
                        && seen.insert(capability.as_str())
                })
        })
}
