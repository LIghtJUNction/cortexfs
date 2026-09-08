use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::agent::compactstrategy::CompactStrategy;
use crate::agent::prompt::compact::format_history_with_strategy;
use crate::runtime::compactabi::{CompactInvocation, MAX_COMPACT_INPUT_BYTES, compact_frame};
use crate::runtime::run_custom_compact;
use crate::tests::helpers::unix_identity_for;
use cortexfs_context::Message;

const INVOCATION: CompactInvocation<'static> = CompactInvocation {
    agent: "executor",
    session: "default",
    max_chars: 80,
};

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
    let identity = unix_identity_for(root.path())?;
    let history = format_history_with_strategy(
        messages,
        CompactStrategy::Summarize,
        root.path(),
        &INVOCATION,
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
    let identity = unix_identity_for(root.path())?;
    let summary = run_custom_compact(
        &phase.join("mycompact"),
        &INVOCATION,
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
fn custom_compact_deadline_includes_blocked_stdin() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let hook = root.path().join("blocked");
    write_hook(&hook, "sleep 8", 0o700)?;
    let overhead = compact_frame(&INVOCATION, &[Message::new("user", "")]).len();
    let messages = [Message::new(
        "user",
        "x".repeat(MAX_COMPACT_INPUT_BYTES - overhead),
    )];
    assert_eq!(
        compact_frame(&INVOCATION, &messages).len(),
        MAX_COMPACT_INPUT_BYTES
    );
    let result = run_custom_compact(
        &hook,
        &INVOCATION,
        &messages,
        &unix_identity_for(root.path())?,
    );
    assert_eq!(result.err().map(|error| error.code()), Some("ETIMEDOUT"));
    Ok(())
}

#[test]
fn custom_compact_empty_or_failed_summary_falls_back_to_recent_history()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("compact.d"))?;
    let messages = "{\"role\":\"user\",\"content\":\"first message detail\"}\n{\"role\":\"user\",\"content\":\"latest message detail\"}";
    let invocation = CompactInvocation {
        max_chars: 35,
        ..INVOCATION
    };
    let identity = unix_identity_for(root.path())?;
    let expected = format_history_with_strategy(
        messages,
        CompactStrategy::Truncate,
        root.path(),
        &invocation,
        &identity,
    );
    for body in ["cat >/dev/null", "exit 1"] {
        write_hook(&root.path().join("compact.d/custom"), body, 0o700)?;
        let rendered = format_history_with_strategy(
            messages,
            CompactStrategy::Custom("custom".into()),
            root.path(),
            &invocation,
            &identity,
        );
        assert_eq!(rendered, expected);
        assert!(rendered.contains("latest message detail"));
    }
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
