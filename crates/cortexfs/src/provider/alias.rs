use std::path::{Path, PathBuf};

use super::{ProjectedProviderModel, ProviderSnapshot};
use crate::{DEFAULT_MODEL_ALIAS, HELPER_MODEL_ALIAS};

pub fn current_model_alias_target(
    alias: &str,
    existing: Option<&Path>,
    snapshot: &ProviderSnapshot,
) -> PathBuf {
    let models = snapshot.models();
    if let Some(existing) = existing.filter(|target| is_current_model_alias_target(target, models))
    {
        return existing.to_path_buf();
    }
    let preferred = match alias {
        DEFAULT_MODEL_ALIAS => Some(cortexfs_paths::model_path(
            &cortexfs_paths::ctx_root(),
            "openai",
            "gpt-5.6",
        )),
        HELPER_MODEL_ALIAS => Some(cortexfs_paths::model_path(
            &cortexfs_paths::ctx_root(),
            "openai",
            "gpt-5.6-sol",
        )),
        _ => None,
    };
    let selected = (alias == DEFAULT_MODEL_ALIAS)
        .then(|| configured_default_model(snapshot))
        .flatten()
        .or_else(|| {
            preferred.and_then(|target| models.iter().find(|model| model_target(model) == target))
        })
        .or_else(|| {
            (alias == HELPER_MODEL_ALIAS)
                .then(|| models.iter().find(|model| model.model == "gpt-5.6-sol"))
                .flatten()
        })
        .or_else(|| capability_model(alias, models))
        .or_else(|| models.first());
    selected.map_or_else(
        || cortexfs_paths::model_path(&cortexfs_paths::ctx_root(), "debug", "echo"),
        model_target,
    )
}

fn configured_default_model(snapshot: &ProviderSnapshot) -> Option<&ProjectedProviderModel> {
    snapshot
        .configs()
        .iter()
        .filter(|entry| entry.1.enabled)
        .filter_map(|entry| {
            let provider = &entry.0;
            let config = &entry.1;
            let default = config.default_model.as_deref()?;
            snapshot
                .models()
                .iter()
                .find(|model| model.provider == *provider && model.model == default)
                .map(|model| (provider, model))
        })
        .min_by(|left, right| {
            left.0
                .cmp(right.0)
                .then_with(|| left.1.model.cmp(&right.1.model))
        })
        .map(|(_provider, model)| model)
}

pub fn is_current_model_alias_target(target: &Path, models: &[ProjectedProviderModel]) -> bool {
    target == cortexfs_paths::model_path(&cortexfs_paths::ctx_root(), "debug", "echo")
        || models.iter().any(|model| target == model_target(model))
}

fn model_target(model: &ProjectedProviderModel) -> PathBuf {
    cortexfs_paths::model_path(&cortexfs_paths::ctx_root(), &model.provider, &model.model)
}

fn capability_model<'a>(
    alias: &str,
    models: &'a [ProjectedProviderModel],
) -> Option<&'a ProjectedProviderModel> {
    models.iter().find(|model| match alias {
        "fast" => has_word(&model.model, "fast"),
        "reason" => model.cap.lines().any(|cap| cap.trim() == "reasoning"),
        "code" => ["code", "executor", "coding"]
            .iter()
            .any(|word| has_word(&model.model, word)),
        "vision" => model
            .cap
            .lines()
            .any(|cap| matches!(cap.trim(), "vision" | "image_input")),
        _ => false,
    })
}

fn has_word(model: &str, expected: &str) -> bool {
    model
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case(expected))
}
