use crate::CliError;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub(super) fn write_manifest(
    staging: &Path,
    object: (&str, &str),
    executable: &Path,
    controls: &BTreeMap<String, String>,
    version: Option<&str>,
    integrity: (Option<&str>, bool),
) -> Result<PathBuf, CliError> {
    let (class, name) = object;
    let (expected_sha256, require_hash) = integrity;
    let sha256 = executable_sha256(executable)?;
    if expected_sha256.is_some_and(|expected| expected != sha256) {
        return Err(CliError::usage(format!(
            "package executable sha256 mismatch: {class}/{name}"
        )));
    }
    if require_hash && expected_sha256.is_none() {
        return Err(CliError::usage(format!(
            "package executable sha256 is required: {class}/{name}"
        )));
    }
    let manifest = staging.join(format!("{class}-{name}.json"));
    let schema = version.map_or("cortexfs.object/v1", |_| "cortexfs.object/v2");
    let mut value = json!({
        "schema": schema,
        "class": class,
        "name": name,
        "executable": {"path": executable, "sha256": sha256},
        "controls": controls,
    });
    if let Some(version) = version {
        let object = value
            .as_object_mut()
            .ok_or_else(|| CliError::unavailable("cannot build package manifest"))?;
        object.extend([
            ("version".to_owned(), json!(version)),
            (
                "compatibility".to_owned(),
                json!({"cortexfs": format!(">={}, <0.2.0", env!("CARGO_PKG_VERSION"))}),
            ),
        ]);
    }
    let text = serde_json::to_vec(&value).map_err(|error| {
        CliError::unavailable(format!("cannot encode package manifest: {error}"))
    })?;
    fs::write(&manifest, text).map_err(|error| {
        CliError::unavailable(format!("cannot stage package manifest: {error}"))
    })?;
    Ok(manifest)
}

fn executable_sha256(path: &Path) -> Result<String, CliError> {
    let mut file = cortexfs::support::plain::open_plain_file(path).map_err(|error| {
        CliError::usage(format!(
            "cannot open package executable {}: {error}",
            path.display()
        ))
    })?;
    let metadata = file.metadata().map_err(|error| {
        CliError::usage(format!(
            "cannot inspect package executable {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(CliError::usage(format!(
            "package executable is not executable: {}",
            path.display()
        )));
    }
    cortexfs::object::receipt::executable_sha256(&mut file, None).map_err(|error| {
        CliError::unavailable(error.message().replacen(
            "cannot read executable",
            "cannot hash package executable",
            1,
        ))
    })
}
