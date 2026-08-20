use std::path::Path;

use cortexfs_metadatas::{ContextWindowPolicy, MetadataCatalog};
use serde_json::Value;

use crate::provider::ProviderSnapshot;
use crate::{FuseProjection, is_model_name};

impl FuseProjection {
    pub(crate) fn context_policy_for_session_path(
        &self,
        path: &str,
    ) -> Option<ContextWindowPolicy> {
        if !Self::is_session_raw_path(path) {
            return None;
        }
        let session = self.resolve(path).ok()?.parent()?.to_owned();
        let metadata = read_session_model(&session)?;
        let reference = self.resolve_model_reference(&metadata)?;
        let catalog = MetadataCatalog::from_cache_or_empty(&self.provider_model_cache_dir);
        let short = reference.split_once('/').map(|(_, model)| model);
        short
            .and_then(|model| catalog.resolve(model))
            .or_else(|| catalog.resolve(&reference))
            .map(cortexfs_metadatas::ModelMetadata::context_policy)
    }

    fn resolve_model_reference(&self, reference: &str) -> Option<String> {
        if is_model_name(reference) {
            return Some(reference.to_owned());
        }
        let snapshot =
            ProviderSnapshot::load(&self.provider_config_dir, &self.provider_model_cache_dir)
                .ok()?;
        let target = self.default_model_alias_target(reference, &snapshot).ok()?;
        model_reference_from_target(&target)
    }
}

fn read_session_model(session: &Path) -> Option<String> {
    let content =
        crate::support::plain::read_small_text_file(&session.join("meta.json"), 64 * 1024).ok()?;
    serde_json::from_str::<Value>(&content)
        .ok()?
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .map(str::to_owned)
}

fn model_reference_from_target(target: &Path) -> Option<String> {
    let parts = target
        .iter()
        .filter_map(|part| part.to_str())
        .collect::<Vec<_>>();
    let index = parts.iter().position(|part| *part == "model")?;
    let provider = parts.get(index + 1)?;
    let model = parts.get(index + 2)?;
    is_model_name(&format!("{provider}/{model}")).then(|| format!("{provider}/{model}"))
}
