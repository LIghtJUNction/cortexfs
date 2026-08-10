use crate::*;
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};

pub(crate) const PACKAGE_SCHEMA: &str = "cortexfs.package/v1";
const PACKAGE_FILES: &[&str] = &["cortexfs.yaml", "cortexfs.yml", "cortexfs.json"];
const MAX_PACKAGE_BYTES: u64 = 1024 * 1024;
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageDocument {
    #[serde(default)]
    pub(crate) schema: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) tools: Vec<PackageTool>,
    #[serde(default)]
    pub(crate) agents: Vec<PackageAgent>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageTool {
    pub(crate) name: String,
    #[serde(alias = "executable")]
    pub(crate) run: PathBuf,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) schema: Option<String>,
    #[serde(default)]
    pub(crate) cap: Option<String>,
    #[serde(default)]
    pub(crate) policy: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageAgent {
    pub(crate) name: String,
    #[serde(alias = "executable")]
    pub(crate) run: PathBuf,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) instructions: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) tools: Vec<String>,
    #[serde(default)]
    pub(crate) parent: Option<String>,
}
pub(crate) struct Package {
    pub(crate) root: PathBuf,
    pub(crate) document: PackageDocument,
}

#[expect(
    clippy::verbose_file_reads,
    reason = "the no-follow file descriptor keeps package input off symlink paths"
)]
pub(crate) fn load_package(spec: &Path) -> Result<Package, CliError> {
    let manifest = resolve_package_manifest(spec)?;
    let mut file = cortexfs::support::plain::open_plain_file(&manifest).map_err(|error| {
        CliError::usage(format!(
            "cannot open package {}: {error}",
            manifest.display()
        ))
    })?;
    let metadata = file.metadata().map_err(|error| {
        CliError::usage(format!(
            "cannot inspect package {}: {error}",
            manifest.display()
        ))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_PACKAGE_BYTES {
        return Err(CliError::usage(
            "package manifest must be a regular file no larger than 1 MiB",
        ));
    }
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|error| {
        CliError::usage(format!(
            "cannot read package {}: {error}",
            manifest.display()
        ))
    })?;
    let document: PackageDocument = serde_yaml::from_str(&text)
        .map_err(|error| CliError::usage(format!("invalid CortexFS package: {error}")))?;
    super::check::validate_package(&document)?;
    Ok(Package {
        root: manifest
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
        document,
    })
}

fn resolve_package_manifest(spec: &Path) -> Result<PathBuf, CliError> {
    if spec.is_file() {
        return Ok(spec.to_path_buf());
    }
    if spec.is_dir() {
        for file in PACKAGE_FILES {
            let candidate = spec.join(file);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        return Err(CliError::usage(format!(
            "no cortexfs.yaml in package directory {}",
            spec.display()
        )));
    }
    Err(CliError::usage(format!(
        "cannot read package {}: No such file or directory",
        spec.display()
    )))
}
