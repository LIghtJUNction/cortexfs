use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

use super::catalog::cached_model_limits;
use super::config::{ProjectedProviderModel, ProviderConfig};
use super::discovery::provider_cached_models;
use crate::{ModelContextLimit, STABLE_MODEL_CAPABILITIES, is_object_name};

pub(super) fn project_models(
    provider: &str,
    config: &ProviderConfig,
    cache_dir: &Path,
    projected: &mut Vec<ProjectedProviderModel>,
) {
    let limits = cached_model_limits(cache_dir);
    let driver = driver_text(&config.formats);
    for model in model_names(config, cache_dir, provider) {
        if projected
            .iter()
            .any(|known| known.provider == provider && known.model == model)
        {
            continue;
        }
        let limit = config
            .model_limits
            .get(&model)
            .or_else(|| limits.get(&format!("{provider}/{model}")))
            .copied()
            .and_then(ModelContextLimit::known)
            .unwrap_or(ModelContextLimit::Unknown);
        let cap = capability_text(
            &config.formats,
            config.model_capabilities.get(&model).map(Vec::as_slice),
        );
        projected.push(ProjectedProviderModel {
            provider: provider.to_owned(),
            model,
            base_url: config.base_url.trim().to_owned(),
            driver: driver.clone(),
            cap,
            effort: "auto".to_owned(),
            fallback: fallback(provider, config.default_model.as_deref()),
            limit,
        });
    }
}

fn model_names(config: &ProviderConfig, cache: &Path, provider: &str) -> Vec<String> {
    let cached = provider_cached_models(cache, provider);
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for model in config
        .default_model
        .iter()
        .chain(&config.models)
        .map(String::as_str)
        .chain(cached.iter().map(String::as_str))
    {
        let model = model.trim();
        if is_object_name(model) && seen.insert(model.to_owned()) {
            names.push(model.to_owned());
        }
    }
    names
}

fn driver_text(formats: &[String]) -> String {
    let responses = formats
        .iter()
        .any(|value| value.trim() == "openai.responses");
    let chat = formats.iter().any(|value| value.trim() == "openai.chat") || !responses;
    let default = if chat {
        "openai-chat"
    } else {
        "openai-responses"
    };
    let agent = if responses && chat {
        "openai-responses,openai-chat"
    } else {
        default
    };
    format!("default={default}\nexec={default}\nagent={agent}\n")
}

fn capability_text(formats: &[String], configured: Option<&[String]>) -> String {
    if let Some(configured) = configured {
        return STABLE_MODEL_CAPABILITIES
            .iter()
            .filter(|capability| configured.iter().any(|value| value == **capability))
            .fold(String::new(), |mut output, capability| {
                let _ignored = writeln!(output, "{capability}");
                output
            });
    }
    let tools = formats
        .iter()
        .any(|value| value.trim() == "openai.responses");
    if tools {
        "chat\nstream\ntool_call_syntax\n"
    } else {
        "chat\nstream\n"
    }
    .to_owned()
}

fn fallback(provider: &str, default: Option<&str>) -> String {
    default
        .filter(|model| is_object_name(model))
        .map_or_else(String::new, |model| format!("{provider}/{model}\n"))
}

pub fn projected_control_content(model: &ProjectedProviderModel, file: &str) -> Option<String> {
    match file {
        "id" => Some(format!("{}/{}\n", model.provider, model.model)),
        "driver" => Some(model.driver.clone()),
        "cap" => Some(model.cap.clone()),
        "effort" => Some(format!("{}\n", model.effort)),
        "fallback" => Some(model.fallback.clone()),
        "limit" => Some(format!("{}\n", model.limit)),
        "default" => Some(format!("base_url={}\n", model.base_url)),
        "session" => Some("none\n".to_owned()),
        "status" => Some("configured\n".to_owned()),
        "log" => Some("\n".to_owned()),
        _ => None,
    }
}
