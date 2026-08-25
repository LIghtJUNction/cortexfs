use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::AgentUnixIdentity;
use crate::runtime::hookabi::{HookInvocation, HookPhase};
use crate::runtime::hooks::run_agent_hooks;

fn write_hook(path: &std::path::Path, body: &str, mode: u32) -> std::io::Result<()> {
    fs::write(path, format!("#!/bin/sh\n{body}\n"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[test]
fn agent_hooks_run_in_order_and_receive_bounded_metadata() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let phase = root.path().join("hooks/pre.d");
    fs::create_dir_all(&phase)?;
    write_hook(&phase.join("01-check"), "grep -q 'cortexfs.hook/v1'", 0o700)?;
    write_hook(&phase.join("02-ok"), "exit 0", 0o700)?;
    let identity = AgentUnixIdentity::new(
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        [],
    );
    run_agent_hooks(
        root.path(),
        &HookInvocation {
            phase: HookPhase::Pre,
            action: "model",
            agent: "executor",
            run: "run-1",
            step: 2,
            tool: None,
            status: None,
        },
        &identity,
    )?;
    Ok(())
}

#[test]
fn agent_hooks_fail_closed_for_nonzero_exit() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let phase = root.path().join("hooks/post.d");
    fs::create_dir_all(&phase)?;
    write_hook(&phase.join("reject"), "exit 7", 0o700)?;
    let identity = AgentUnixIdentity::new(
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        [],
    );
    let error = match run_agent_hooks(
        root.path(),
        &HookInvocation {
            phase: HookPhase::Post,
            action: "model",
            agent: "executor",
            run: "run-1",
            step: 0,
            tool: None,
            status: Some("ok"),
        },
        &identity,
    ) {
        Ok(()) => return Err("non-zero hook unexpectedly succeeded".into()),
        Err(error) => error,
    };
    assert_eq!(error.code(), "EACCES");
    assert!(error.message().contains("reject"));
    Ok(())
}
