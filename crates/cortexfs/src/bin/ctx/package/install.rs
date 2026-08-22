use super::manifest::load_package;
use super::source::{canonical_source, default_package_source};
use super::write::{ensure_targets_absent, write_manifests};
use crate::{
    CliError, Command, ensure_reference_tree, is_mount_point, print_line, required_arg,
    terminal_safe_field,
};
use cortexfs::object::install::{InstallTier, install_object};
use std::path::{Path, PathBuf};

pub(crate) fn parse_package_install_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let mut package = None;
    let mut source = None;
    let mut tier = InstallTier::System;
    let mut check = false;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--source" => {
                source = Some(PathBuf::from(required_arg(
                    &mut values,
                    "install --source requires a path",
                )?));
            }
            "--check" => check = true,
            "--tier" => {
                let value = required_arg(&mut values, "install --tier requires user or system")?;
                tier = InstallTier::parse(&value)
                    .ok_or_else(|| CliError::usage("install --tier expects user or system"))?;
            }
            _ if value.starts_with('-') => {
                return Err(CliError::usage(format!(
                    "unexpected install argument: {}",
                    terminal_safe_field(&value)
                )));
            }
            _ if package.is_none() => package = Some(PathBuf::from(value)),
            _ => return Err(CliError::usage("install accepts one package path")),
        }
    }
    if check && source.is_some() {
        return Err(CliError::usage("install --check does not accept --source"));
    }
    Ok(Command::PackageInstall {
        package: package.ok_or_else(|| CliError::usage("install requires a package path"))?,
        source,
        tier,
        check,
    })
}

pub(crate) fn run_package_install(
    spec: &Path,
    source: Option<&Path>,
    tier: InstallTier,
    check: bool,
) -> Result<(), CliError> {
    let package = load_package(spec)?;
    if tier == InstallTier::User && !package.document.agents.is_empty() {
        return Err(CliError::usage(
            "package agents require --tier system; tools may use --tier user",
        ));
    }
    let requested_source = source.map(Path::to_path_buf);
    let temp = tempfile::tempdir().map_err(|error| {
        CliError::unavailable(format!("cannot create package staging directory: {error}"))
    })?;
    let manifests = write_manifests(&package, temp.path())?;
    for manifest in &manifests {
        cortexfs::object::install::check_object(manifest).map_err(|error| {
            CliError::usage(format!("invalid package object: {}", error.message()))
        })?;
    }
    if check {
        print_line("package valid")?;
        return Ok(());
    }
    let source = requested_source.map_or_else(default_package_source, Ok)?;
    let source = canonical_source(&source)?;
    if is_mount_point(&source).unwrap_or(false) {
        return Err(CliError::usage(format!(
            "package source is a mountpoint, choose its durable backing tree: {}",
            source.display()
        )));
    }
    ensure_reference_tree(&source).map_err(|error| {
        CliError::unavailable(format!("cannot prepare package source: {}", error.errno()))
    })?;
    ensure_targets_absent(&source, &package, tier)?;
    for manifest in manifests {
        let installed = install_object(&source, &manifest, tier).map_err(|error| {
            CliError::unavailable(format!(
                "cannot install package object: {}",
                error.message()
            ))
        })?;
        print_line(&format!(
            "installed {}/{}",
            installed.class.as_str(),
            installed.name
        ))?;
    }
    Ok(())
}
