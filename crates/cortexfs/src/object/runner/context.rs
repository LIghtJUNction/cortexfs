use super::*;
use std::path::Path;

pub(crate) fn model_effort(ctx_root: &Path, provider: &str, model: &str) -> cortexfs::ModelEffort {
    let path = cortexfs_paths::model_control_path(ctx_root, provider, model).join("effort");
    read_small_plain_text_file(&path, MAX_RUNNER_CONTROL_BYTES, "runner")
        .ok()
        .and_then(|content| cortexfs::ModelEffort::parse(&content))
        .unwrap_or(cortexfs::ModelEffort::Auto)
}
