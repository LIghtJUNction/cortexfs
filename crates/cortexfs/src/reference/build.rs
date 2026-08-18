use std::ffi::OsStr;
use std::fs;

use crate::object::bootstrap::stage_generated_model_pair;
use crate::provider::projected_control_content;
use crate::reference::provenance;
use crate::support::plain::open_directory_at;
use crate::support::receipt::{EntryKind, EntryReceipt, receipt_at};
use crate::{
    MODEL_CONTROL_FILES, ProjectedProviderModel, REFERENCE_OBJECT_RUNNER, ReferenceTreeError,
};

pub fn build_provider(
    stage: &fs::File,
    provider: &str,
    models: &[&ProjectedProviderModel],
) -> Result<(fs::File, EntryReceipt), ReferenceTreeError> {
    nix::sys::stat::mkdirat(
        stage,
        "next",
        nix::sys::stat::Mode::from_bits_truncate(0o755),
    )
    .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let desired = open_directory_at(stage, OsStr::new("next"))
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    nix::sys::stat::fchmod(&desired, nix::sys::stat::Mode::from_bits_truncate(0o755))
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    for model in models {
        let controls = MODEL_CONTROL_FILES
            .iter()
            .map(|file| projected_control_content(model, file).map(|value| (*file, value)))
            .collect::<Option<Vec<_>>>()
            .ok_or(ReferenceTreeError::CannotCreate)?;
        let overrides = controls
            .iter()
            .map(|&(file, ref value)| (file, value.as_str()))
            .collect::<Vec<_>>();
        stage_generated_model_pair(
            &desired,
            &model.model,
            &format!("{provider}/{}", model.model),
            REFERENCE_OBJECT_RUNNER,
            &overrides,
        )
        .map_err(ReferenceTreeError::Object)?;
    }
    provenance::seal(&desired).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let receipt = receipt_at(stage, "next", EntryKind::Directory)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?
        .ok_or(ReferenceTreeError::CannotCreate)?;
    Ok((desired, receipt))
}
