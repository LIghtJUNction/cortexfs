use crate::{CliError, adopt_default_source_root};
use std::env;
use std::path::{Path, PathBuf};

pub(super) fn default_package_source() -> Result<PathBuf, CliError> {
    if let Some(source) = env::var_os("CTX_SOURCE") {
        if source.is_empty() {
            return Err(CliError::usage("CTX_SOURCE must not be empty"));
        }
        return Ok(PathBuf::from(source));
    }
    let parent = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| CliError::unavailable("cannot choose package source without HOME"))?;
    adopt_default_source_root(&parent.join("cortexfs"))
}

pub(super) fn canonical_source(source: &Path) -> Result<PathBuf, CliError> {
    if !source.exists() {
        return Ok(source.to_path_buf());
    }
    source.canonicalize().map_err(|error| {
        CliError::unavailable(format!(
            "cannot resolve package source {}: {error}",
            source.display()
        ))
    })
}
