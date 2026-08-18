use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::object::install::create_stage;
use crate::object::residue::cleanup_residue;
use crate::provider::name::is_reserved_provider_name;
use crate::reference::build::build_provider;
use crate::reference::provenance;
use crate::support::plain::open_directory_at;
use crate::support::receipt::{
    EntryKind, EntryReceipt, entry_matches, park_entry, publish_entry, restore_entry,
};
use crate::{ProjectedProviderModel, ReferenceTreeError};

pub fn reconcile_provider_directory(
    root: &Path,
    model_root: &fs::File,
    provider: &str,
    models: &[&ProjectedProviderModel],
    existing: Option<EntryReceipt>,
    active: bool,
) -> Result<(), ReferenceTreeError> {
    if !active && existing.is_none() {
        return Ok(());
    }
    if !active && preserve_unmanaged_provider(model_root, provider, existing)? {
        return Ok(());
    }
    let (stage_name, stage, stage_receipt) =
        create_stage(model_root).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let desired = active
        .then(|| build_provider(&stage, provider, models))
        .transpose()?;
    if let Some(old) = existing {
        park_entry(
            model_root,
            provider,
            &stage,
            "old",
            old,
            EntryKind::Directory,
        )
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if !provenance::accept_old(&stage, desired.as_ref().map(|pair| &pair.0), active)
            .unwrap_or(false)
            && !archive_unmanaged_provider(&stage, model_root, provider, &stage_name, old)?
        {
            let restored = restore_entry(
                &stage,
                "old",
                model_root,
                provider,
                old,
                EntryKind::Directory,
            )
            .is_ok();
            if restored {
                let _cleaned = cleanup_stage(root, &stage_name, stage_receipt);
            }
            return Err(ReferenceTreeError::CannotCreate);
        }
    }
    if let Some((desired_dir, desired_receipt)) = desired {
        if publish_entry(
            &stage,
            "next",
            model_root,
            provider,
            desired_receipt,
            EntryKind::Directory,
        )
        .is_err()
        {
            restore_old(&stage, model_root, provider, existing);
            return Err(ReferenceTreeError::CannotCreate);
        }
        let published = open_directory_at(model_root, OsStr::new(provider))
            .map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if provenance::verify(&published).is_err() {
            let parked = park_entry(
                model_root,
                provider,
                &stage,
                "next",
                desired_receipt,
                EntryKind::Directory,
            )
            .is_ok();
            let restored = restore_old(&stage, model_root, provider, existing);
            if parked && restored {
                let _cleaned = cleanup_stage(root, &stage_name, stage_receipt);
            }
            return Err(ReferenceTreeError::CannotCreate);
        }
        drop(desired_dir);
    }
    cleanup_stage(root, &stage_name, stage_receipt)
}

fn archive_unmanaged_provider(
    stage: &fs::File,
    model_root: &fs::File,
    provider: &str,
    stage_name: &str,
    receipt: EntryReceipt,
) -> Result<bool, ReferenceTreeError> {
    let old = open_directory_at(stage, OsStr::new("old"))
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    if provenance::has_manifest(&old).map_err(|_error| ReferenceTreeError::CannotCreate)? {
        return Ok(false);
    }
    let backup = format!(".cortexfs-legacy-{provider}-{stage_name}");
    restore_entry(
        stage,
        "old",
        model_root,
        &backup,
        receipt,
        EntryKind::Directory,
    )
    .map(|()| true)
    .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn preserve_unmanaged_provider(
    model_root: &fs::File,
    provider: &str,
    existing: Option<EntryReceipt>,
) -> Result<bool, ReferenceTreeError> {
    if is_reserved_provider_name(provider) {
        return Ok(false);
    }
    let Some(receipt) = existing else {
        return Ok(false);
    };
    let directory = open_directory_at(model_root, OsStr::new(provider))
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    if !entry_matches(model_root, provider, receipt, EntryKind::Directory) {
        return Err(ReferenceTreeError::CannotCreate);
    }
    provenance::has_manifest(&directory)
        .map(|managed| !managed)
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn restore_old(
    stage: &fs::File,
    model_root: &fs::File,
    provider: &str,
    old: Option<EntryReceipt>,
) -> bool {
    old.is_none_or(|old| {
        restore_entry(
            stage,
            "old",
            model_root,
            provider,
            old,
            EntryKind::Directory,
        )
        .is_ok()
    })
}

fn cleanup_stage(root: &Path, name: &str, receipt: EntryReceipt) -> Result<(), ReferenceTreeError> {
    let path = PathBuf::from("model").join(name);
    cleanup_residue(root, &path, receipt.dev, receipt.ino, true)
        .map(|_| ())
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}
