use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::AgentUnixIdentity;
use crate::agent::compactstrategy::CompactStrategy;
use crate::agent::prompt::compact::format_history_with_strategy;
use crate::runtime::compactabi::CompactInvocation;
use crate::runtime::compactexec::run_custom_compact;
use cortexfs_context::Message;

fn write_hook(path: &Path, body: &str, mode: u32) -> std::io::Result<()> {
    fs::write(path, format!("#!/bin/sh\n{body}\n"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[test]
fn compact_strategy_summarize_inserts_builtin_summary() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("compact.strategy"), "summarize\n")?;
    let messages = concat!(
        "{\"role\":\"user\",\"content\":\"first message with enough detail\"}\n",
        "{\"role\":\"assistant\",\"content\":\"second message with enough detail\"}\n",
        "{\"role\":\"user\",\"content\":\"third message with enough detail\"}\n",
    );
    let identity = AgentUnixIdentity::new(
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        [],
    );
    let history = format_history_with_strategy(
        messages,
        80,
        CompactStrategy::Summarize,
        root.path(),
        "coder",
        "default",
        &identity,
    );
    assert!(history.contains("Summary of earlier context"));
    Ok(())
}

#[test]
fn custom_compact_executable_receives_bounded_history_frame()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let phase = root.path().join("compact.d");
    fs::create_dir_all(&phase)?;
    write_hook(
        &phase.join("mycompact"),
        "grep -q 'cortexfs.compact/v1' && printf 'custom summary'",
        0o700,
    )?;
    let identity = AgentUnixIdentity::new(
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        [],
    );
    let summary = run_custom_compact(
        &phase.join("mycompact"),
        &CompactInvocation {
            agent: "coder",
            session: "default",
            max_chars: 80,
        },
        &[
            Message::new("user", "first message with enough detail"),
            Message::new("assistant", "second message with enough detail"),
        ],
        &identity,
    )?;
    assert_eq!(summary, "custom summary");
    Ok(())
}

#[test]
fn loop_resolve_uses_loop_d_executable_for_custom_loop() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("loop"), "myloop\n")?;
    let phase = root.path().join("loop.d");
    fs::create_dir_all(&phase)?;
    write_hook(&phase.join("myloop"), "exit 0", 0o700)?;
    let default = root.path().join("agent");
    fs::write(&default, b"\0")?;
    let resolved = crate::agent::loopresolve::resolve_agent_loop_executable(root.path(), &default);
    assert_eq!(resolved, phase.join("myloop"));
    Ok(())
}
