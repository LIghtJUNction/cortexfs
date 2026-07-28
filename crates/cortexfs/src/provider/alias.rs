use std::path::{Path, PathBuf};

use super::ProjectedProviderModel;
use crate::{
    DEFAULT_MODEL_ALIAS, DEFAULT_MODEL_ALIAS_TARGET, HELPER_MODEL_ALIAS, HELPER_MODEL_ALIAS_TARGET,
};

pub fn current_model_alias_target(
    alias: &str,
    existing: Option<&Path>,
    models: &[ProjectedProviderModel],
) -> PathBuf {
    if let Some(existing) = existing.filter(|target| is_current_model_alias_target(target, models))
    {
        return existing.to_path_buf();
    }
    let preferred = match alias {
        DEFAULT_MODEL_ALIAS => Some(DEFAULT_MODEL_ALIAS_TARGET),
        HELPER_MODEL_ALIAS => Some(HELPER_MODEL_ALIAS_TARGET),
        _ => None,
    };
    let selected = preferred
        .and_then(|target| {
            models
                .iter()
                .find(|model| model_target(model) == Path::new(target))
        })
        .or_else(|| {
            (alias == HELPER_MODEL_ALIAS)
                .then(|| models.iter().find(|model| model.model == "gpt-5.6-sol"))
                .flatten()
        })
        .or_else(|| capability_model(alias, models))
        .or_else(|| models.first());
    selected.map_or_else(|| PathBuf::from("/ctx/model/debug/echo"), model_target)
}

pub fn is_current_model_alias_target(target: &Path, models: &[ProjectedProviderModel]) -> bool {
    target == Path::new("/ctx/model/debug/echo")
        || models.iter().any(|model| target == model_target(model))
}

fn model_target(model: &ProjectedProviderModel) -> PathBuf {
    PathBuf::from(format!("/ctx/model/{}/{}", model.provider, model.model))
}

fn capability_model<'a>(
    alias: &str,
    models: &'a [ProjectedProviderModel],
) -> Option<&'a ProjectedProviderModel> {
    models.iter().find(|model| match alias {
        "fast" => has_word(&model.model, "fast"),
        "reason" => model.cap.lines().any(|cap| cap.trim() == "reasoning"),
        "code" => ["code", "coder", "coding"]
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
