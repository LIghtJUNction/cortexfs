use crate::CliError;
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};

pub(crate) const PACKAGE_SCHEMA: &str = "cortexfs.package/v1";
const MAX_PACKAGE_BYTES: usize = 1024 * 1024;
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageDocument {
    pub(crate) schema: Option<String>,
    pub(crate) version: Option<String>,
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
    pub(crate) sha256: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) schema: Option<serde_json::Value>,
    pub(crate) cap: Option<String>,
    pub(crate) policy: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageAgent {
    pub(crate) name: String,
    #[serde(alias = "executable")]
    pub(crate) run: PathBuf,
    pub(crate) sha256: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) instructions: Option<String>,
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) tools: Vec<String>,
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
    let file = cortexfs::support::plain::open_plain_file(&manifest).map_err(|error| {
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
    if !metadata.is_file() {
        return Err(CliError::usage("package manifest must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(u64::try_from(MAX_PACKAGE_BYTES.saturating_add(1)).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CliError::usage(format!(
                "cannot read package {}: {error}",
                manifest.display()
            ))
        })?;
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(CliError::usage(
            "package manifest must be a regular file no larger than 1 MiB",
        ));
    }
    let text = String::from_utf8(bytes).map_err(|error| {
        CliError::usage(format!(
            "invalid UTF-8 in package {}: {error}",
            manifest.display()
        ))
    })?;
    let document: PackageDocument = toml::from_str(&text)
        .map_err(|error| CliError::usage(format!("invalid CortexFS package: {error}")))?;
    super::check::validate_package(&document)?;
    let root = manifest
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| CliError::usage(format!("cannot resolve package directory: {error}")))?;
    Ok(Package { root, document })
}

fn resolve_package_manifest(spec: &Path) -> Result<PathBuf, CliError> {
    if spec.is_file() {
        return Ok(spec.to_path_buf());
    }
    if spec.is_dir() {
        let candidate = spec.join("cortexfs.toml");
        return candidate.is_file().then_some(candidate).ok_or_else(|| {
            CliError::usage(format!(
                "no cortexfs.toml in package directory {}",
                spec.display()
            ))
        });
    }
    Err(CliError::usage(format!(
        "cannot read package {}: No such file or directory",
        spec.display()
    )))
}
