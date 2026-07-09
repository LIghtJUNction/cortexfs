use super::*;
use std::path::Path;

pub(crate) fn provider_name_from_config(
    base_url: &str,
    name: Option<&str>,
) -> Result<String, cortexfs::ProviderNameError> {
    cortexfs::provider_name_from_config(base_url, name)
}
pub(crate) fn model_effort(ctx_root: &Path, provider: &str, model: &str) -> cortexfs::ModelEffort {
    let path = ctx_root
        .join("model")
        .join(provider)
        .join(format!("{model}.d"))
        .join("effort");
    read_small_plain_text_file(&path, MAX_RUNNER_CONTROL_BYTES, "runner")
        .ok()
        .and_then(|content| cortexfs::ModelEffort::parse(&content))
        .unwrap_or(cortexfs::ModelEffort::Auto)
}
