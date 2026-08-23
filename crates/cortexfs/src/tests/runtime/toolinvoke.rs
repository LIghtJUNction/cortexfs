use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::tool::{InvokeStrategy, read_invoke_strategy, resolve_tool_invoke_executable};

fn write_hook(path: &std::path::Path, body: &str, mode: u32) -> std::io::Result<()> {
    fs::write(path, format!("#!/bin/sh\n{body}\n"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[test]
fn invoke_strategy_custom_uses_invoke_d_executable() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("invoke.strategy"), "myinvoke\n")?;
    let phase = root.path().join("invoke.d");
    fs::create_dir_all(&phase)?;
    write_hook(&phase.join("myinvoke"), "exit 0", 0o700)?;
    let default = root.path().join("tool");
    fs::write(&default, b"\0")?;
    let resolved = resolve_tool_invoke_executable(root.path(), &default);
    assert_eq!(resolved, phase.join("myinvoke"));
    assert_eq!(
        read_invoke_strategy(root.path()),
        InvokeStrategy::Custom("myinvoke".to_owned())
    );
    Ok(())
}

#[test]
fn invoke_strategy_sdk_maps_to_native_tool_mode() {
    assert_eq!(
        crate::tool::invoke_tool_mode(&InvokeStrategy::Sdk),
        Some("native")
    );
    assert_eq!(
        crate::tool::invoke_tool_mode(&InvokeStrategy::Cli),
        Some("cli")
    );
}
