use crate::reference::inspect::generated_provider_dirs;
use crate::reference::lock::lock_provider_reconciliation;
use crate::reference::stage::reconcile_provider_directory;
use crate::support::plain::{create_plain_dir, open_plain_directory};
use crate::{ProjectedProviderModel, ProviderSnapshot, ReferenceTreeError};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Reconciles each physical provider directory as one authoritative derived
/// projection. The cache-local lock serializes writers without introducing a
/// watcher or runtime background task.
pub fn reconcile_provider_model_tree(
    root: &Path,
    config_dir: &Path,
    cache_dir: &Path,
) -> Result<ProviderSnapshot, ReferenceTreeError> {
    let _lock = lock_provider_reconciliation(cache_dir)?;
    let snapshot = ProviderSnapshot::load(config_dir, cache_dir)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let active = snapshot.active();
    let models = snapshot.models();
    let model_path = cortexfs_paths::model_root_path(root);
    create_plain_dir(&model_path).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let model_root =
        open_plain_directory(&model_path).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let existing = generated_provider_dirs(&model_root)?;
    let mut desired = BTreeMap::<String, Vec<&ProjectedProviderModel>>::new();
    for model in models {
        desired
            .entry(model.provider.clone())
            .or_default()
            .push(model);
    }
    let names = active
        .iter()
        .chain(existing.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for provider in names {
        let wanted = desired.get(&provider).map_or(&[][..], Vec::as_slice);
        reconcile_provider_directory(
            root,
            &model_root,
            &provider,
            wanted,
            existing.get(&provider).copied(),
            active.contains(&provider),
        )?;
    }
    Ok(snapshot)
}
