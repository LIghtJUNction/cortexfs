use crate::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub(super) fn write_manifest(
    staging: &Path,
    class: &str,
    name: &str,
    executable: &Path,
    controls: &BTreeMap<String, String>,
) -> Result<PathBuf, CliError> {
    let sha256 = executable_sha256(executable)?;
    let manifest = staging.join(format!("{class}-{name}.json"));
    let value = json!({
        "schema": "cortexfs.object/v2",
        "version": env!("CARGO_PKG_VERSION"),
        "compatibility": {"cortexfs": format!(">={}, <0.2.0", env!("CARGO_PKG_VERSION"))},
        "class": class,
        "name": name,
        "executable": {"path": executable, "sha256": sha256},
        "controls": controls,
    });
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
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            CliError::unavailable(format!("cannot hash package executable: {error}"))
        })?;
        if count == 0 {
            break;
        }
        let chunk = buffer
            .get(..count)
            .ok_or_else(|| CliError::unavailable("invalid package hash read"))?;
        digest.update(chunk);
    }
    let mut output = String::with_capacity(64);
    for byte in digest.finalize() {
        std::fmt::Write::write_fmt(&mut output, format_args!("{byte:02x}"))
            .map_err(|error| CliError::unavailable(error.to_string()))?;
    }
    Ok(output)
}
