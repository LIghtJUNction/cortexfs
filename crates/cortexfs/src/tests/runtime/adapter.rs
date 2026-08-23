use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::channel::{AdapterStrategy, read_adapter_strategy, resolve_channel_adapter_executable};

fn write_hook(path: &std::path::Path, body: &str, mode: u32) -> std::io::Result<()> {
    fs::write(path, format!("#!/bin/sh\n{body}\n"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[test]
fn adapter_strategy_defaults_to_catalog_family() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let strategy = read_adapter_strategy(root.path(), "telegram.primary");
    assert_eq!(strategy, AdapterStrategy::Catalog("telegram".to_owned()));
    Ok(())
}

#[test]
fn adapter_strategy_custom_uses_adapter_d_executable() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("adapter"), "mydriver\n")?;
    let phase = root.path().join("adapter.d");
    fs::create_dir_all(&phase)?;
    write_hook(&phase.join("mydriver"), "exit 0", 0o700)?;
    let strategy = read_adapter_strategy(root.path(), "discord.primary");
    assert_eq!(strategy, AdapterStrategy::Custom("mydriver".to_owned()));
    assert_eq!(
        resolve_channel_adapter_executable(root.path(), &strategy),
        Some(phase.join("mydriver"))
    );
    Ok(())
}
