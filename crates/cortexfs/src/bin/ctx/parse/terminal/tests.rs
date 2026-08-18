#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "terminal parser tests should fail loudly with their operation"
)]

use super::*;

#[test]
fn parses_agent_backed_terminal_create() {
    let command = parse_terminal_command(
        [
            "create",
            "coder",
            "--session",
            "work",
            "--cwd",
            "/workspace",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    )
    .expect("terminal create");
    let Command::Terminal(TerminalArgs::Create {
        agent,
        session,
        cwd,
    }) = command
    else {
        panic!("unexpected terminal create command");
    };
    assert_eq!(agent, "coder");
    assert_eq!(session, "work");
    assert_eq!(cwd, "/workspace");
}

#[test]
fn parses_terminal_status_and_help() {
    let Command::Terminal(TerminalArgs::Status { id }) = parse_terminal_command(
        ["status", "terminal-coder-default"]
            .map(str::to_owned)
            .to_vec(),
    )
    .expect("terminal status") else {
        panic!("unexpected terminal status command");
    };
    assert_eq!(id, "terminal-coder-default");
    let Command::HelpTopic(topic) =
        parse_terminal_command(["watch", "--help"].map(str::to_owned).to_vec())
            .expect("terminal help")
    else {
        panic!("unexpected terminal help command");
    };
    assert_eq!(topic, "terminal watch");
}
