use crate::provider::name::is_reserved_provider_name;
use crate::reference::bootstrap::REFERENCE_OBJECT_RUNNER;
use crate::reference::provenance;
use crate::support::plain::{open_directory_at, proc_fd_path, read_small_text_file_at};
use crate::support::receipt::{EntryKind, EntryReceipt, receipt_at};
use crate::{
    DEBUG_ECHO_MODEL, DEBUG_ECHO_PROVIDER, MODEL_ALIASES, MODEL_CONTROL_FILES, MODEL_ROUTE_FILE,
    ObjectClass, ReferenceTreeError, debug_model_control_content, executable_wrapper_script,
    is_object_name,
};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;

/// Receipts physical provider directories without inspecting their contents
/// while the names remain public. Content verification happens after parking.
pub fn generated_provider_dirs(
    model_root: &fs::File,
) -> Result<BTreeMap<String, EntryReceipt>, ReferenceTreeError> {
    let entries = fs::read_dir(proc_fd_path(model_root))
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let mut providers = BTreeMap::new();
    for entry in entries {
        let name = entry
            .map_err(|_error| ReferenceTreeError::CannotCreate)?
            .file_name()
            .into_string()
            .map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if !is_object_name(&name) {
            continue;
        }
        let directory = match open_directory_at(model_root, OsStr::new(&name)) {
            Ok(directory) => directory,
            Err(_) if name == MODEL_ROUTE_FILE && has_kind(model_root, &name, EntryKind::File)? => {
                continue;
            }
            Err(_)
                if MODEL_ALIASES.contains(&name.as_str())
                    && has_kind(model_root, &name, EntryKind::Symlink)? =>
            {
                continue;
            }
            Err(_) => return Err(ReferenceTreeError::CannotCreate),
        };
        if is_reserved_provider_name(&name) && provenance::verify(&directory).is_err() {
            if name == DEBUG_ECHO_PROVIDER && canonical_debug(&directory) {
                continue;
            }
            return Err(ReferenceTreeError::CannotCreate);
        }
        let receipt = receipt_at(model_root, &name, EntryKind::Directory)
            .map_err(|_error| ReferenceTreeError::CannotCreate)?
            .ok_or(ReferenceTreeError::CannotCreate)?;
        providers.insert(name, receipt);
    }
    Ok(providers)
}

fn has_kind(parent: &fs::File, name: &str, kind: EntryKind) -> Result<bool, ReferenceTreeError> {
    receipt_at(parent, name, kind)
        .map(|receipt| receipt.is_some())
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn canonical_debug(directory: &fs::File) -> bool {
    let Ok(mut names) = fs::read_dir(proc_fd_path(directory)).map(|entries| {
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>()
    }) else {
        return false;
    };
    names.sort();
    if names != ["echo", "echo.d"] {
        return false;
    }
    let expected_wrapper = executable_wrapper_script(
        ObjectClass::Model,
        DEBUG_ECHO_MODEL,
        REFERENCE_OBJECT_RUNNER,
    );
    if read_small_text_file_at(directory, "echo", 64 * 1024, "invalid")
        .ok()
        .as_deref()
        != Some(expected_wrapper.as_str())
        || !matches!(receipt_at(directory, "echo", EntryKind::File), Ok(Some(_)))
    {
        return false;
    }
    let Ok(control) = open_directory_at(directory, "echo.d".as_ref()) else {
        return false;
    };
    MODEL_CONTROL_FILES.iter().all(|file| {
        let expected = if matches!(*file, "default" | "log") {
            Some(String::new())
        } else {
            debug_model_control_content(DEBUG_ECHO_MODEL, file)
        };
        let receipt = matches!(receipt_at(&control, file, EntryKind::File), Ok(Some(_)));
        let actual = read_small_text_file_at(&control, file, 64 * 1024, "invalid").ok();
        receipt && actual == expected
    })
}
