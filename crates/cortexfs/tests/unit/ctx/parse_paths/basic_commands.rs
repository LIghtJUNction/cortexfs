use std::fmt::Write;

fn parse_cli_args(args: &[&str]) -> Result<Cli, CliError> {
    parse(args.iter().map(std::ffi::OsString::from).collect())
}

#[test]
fn parses_leading_root_global_option_only() {
    let cli = parse_cli_args(&["--root", "/tmp/ctx-alt", "status"]);
    assert!(matches!(
        cli,
        Ok(Cli {
            ref root,
            command: Command::Status,
        }) if root == Path::new("/tmp/ctx-alt")
    ));

    let exec = parse_cli_args(&["exec", "agent/coder", "--root", "/tmp/ctx-alt"]);
    assert!(matches!(
        exec,
        Ok(Cli {
            command: Command::Exec { ref path, ref args },
            ..
        }) if path == "agent/coder"
            && args == &["--root".to_owned(), "/tmp/ctx-alt".to_owned()]
    ));

    let tool = parse_cli_args(&["tool", "tsh.config", "--root"]);
    assert!(matches!(
        tool,
        Ok(Cli {
            command: Command::Tool { ref name, ref args },
            ..
        }) if name == "tsh.config" && args == &["--root".to_owned()]
    ));
}

#[test]
fn zero_arg_commands_reject_extra_arguments() {
    let status = cmd!("status", "extra");
    assert!(matches!(
        status,
        Err(ref error) if error.code == 2 && error.message == "unexpected argument: extra"
    ));

    let abi = cmd!("abi", "extra");
    assert!(matches!(
        abi,
        Err(ref error) if error.code == 2 && error.message == "unexpected argument: extra"
    ));
}

#[test]
fn parses_spec_which_command() {
    let command = cmd!("which", "tool", "fs.read");
    assert!(matches!(
        command,
        Ok(Command::Which(ObjectClass::Tool, ref name)) if name == "fs.read"
    ));
}

#[test]
fn parses_top_level_agent_inspect_command() {
    for target in ["agent/coder", "/ctx/agent/coder"] {
        let command = parse_command(vec![
            "inspect".to_owned(),
            target.to_owned(),
            "--session".to_owned(),
            "debug".to_owned(),
        ]);
        assert!(matches!(
            command,
            Ok(Command::Agent(AgentArgs::Inspect {
                ref name,
                session: Some(ref session)
            })) if name == "coder" && session == "debug"
        ));
    }

    let model = cmd!("inspect", "model/main");
    assert!(matches!(
        model,
        Err(ref error) if error.message == "inspect expects agent/NAME"
    ));
}

#[test]
fn parses_man_command() {
    let index = cmd!("man");
    assert!(matches!(index, Ok(Command::Man { topic: None })));

    let agent = cmd!("man", "agent");
    assert!(matches!(
        agent,
        Ok(Command::Man { topic: Some(ref topic) }) if topic == "agent"
    ));

    let extra = cmd!("man", "agent", "extra");
    assert!(matches!(
        extra,
        Err(ref error) if error.code == 2 && error.message == "unexpected argument: extra"
    ));
}

#[test]
fn parses_top_level_file_content_commands() {
    let cat = cmd!("cat", "agent/coder.d/cwd");
    assert!(matches!(
        cat,
        Ok(Command::Cat { ref path }) if path == "agent/coder.d/cwd"
    ));

    let set = cmd!("set", "agent/coder.d/cwd", "/work");
    assert!(matches!(
        set,
        Ok(Command::Set { ref path, ref value }) if path == "agent/coder.d/cwd" && value == "/work"
    ));

    let append = cmd!("append", "agent/coder.d/path", "/ctx/tool");
    assert!(matches!(
        append,
        Ok(Command::Append { ref path, ref value })
            if path == "agent/coder.d/path" && value == "/ctx/tool"
    ));
}

#[test]
fn parses_file_metadata_command() {
    let explicit = cmd!("file", "type", "tool/fs.read");
    assert!(matches!(
        explicit,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Type
                && args.path == "tool/fs.read"
    ));

    let shorthand = cmd!("file", "tool/fs.read");
    assert!(matches!(
        shorthand,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Info
                && args.path == "tool/fs.read"
    ));
}

#[test]
fn parses_schedule_status_command() {
    let command = cmd!(
        "schedule",
        "status",
        "home/1000/agent/base/session/default/context/plan.json",
        "--done",
        "plan",
    );
    assert!(matches!(
        command,
        Ok(Command::Schedule(ScheduleArgs::Status {
            ref path,
            ref done,
        })) if path == "home/1000/agent/base/session/default/context/plan.json"
            && done == &["plan".to_owned()]
    ));

    let missing_done = cmd!(
        "schedule",
        "status",
        "home/1000/agent/base/session/default/context/plan.json",
        "--done",
    );
    assert!(matches!(
        missing_done,
        Err(ref error)
            if error.code == 2
                && error.message == "schedule status --done requires a node id"
    ));
}

#[test]
fn parses_schedule_advance_command() {
    let command = cmd!(
        "schedule",
        "advance",
        "home/1000/agent/base/session/default/context/plan.json",
        "--done",
        "plan",
        "--done",
        "lint",
    );
    assert!(matches!(
        command,
        Ok(Command::Schedule(ScheduleArgs::Advance {
            ref path,
            ref done,
        })) if path == "home/1000/agent/base/session/default/context/plan.json"
            && done == &["plan".to_owned(), "lint".to_owned()]
    ));

    let missing_done = cmd!(
        "schedule",
        "advance",
        "home/1000/agent/base/session/default/context/plan.json",
        "--done",
    );
    assert!(matches!(
        missing_done,
        Err(ref error)
            if error.code == 2
                && error.message == "schedule advance --done requires a node id"
    ));
}

#[test]
fn parses_schedule_claim_command() {
    let command = cmd!(
        "schedule",
        "claim",
        "home/1000/agent/base/session/default/context/plan.json",
        "work-123",
    );
    assert!(matches!(
        command,
        Ok(Command::Schedule(ScheduleArgs::Claim {
            ref path,
            ref child,
        })) if path == "home/1000/agent/base/session/default/context/plan.json"
            && child == "work-123"
    ));

    let missing_child = cmd!(
        "schedule",
        "claim",
        "home/1000/agent/base/session/default/context/plan.json",
    );
    assert!(matches!(
        missing_child,
        Err(ref error)
            if error.code == 2
                && error.message == "schedule claim requires a child name"
    ));
}

#[test]
fn parses_schedule_result_command() {
    let command = cmd!(
        "schedule",
        "result",
        "home/1000/agent/base/session/default/context/plan.json",
        "work-123",
        "done",
        "implemented",
        "--refs-jsonl",
        "{\"id\":\"ref-abc\",\"path\":\"artifact/output.md\",\"kind\":\"artifact\",\"summary\":\"changed\"}\n",
    );
    assert!(matches!(
        command,
        Ok(Command::Schedule(ScheduleArgs::Result {
            ref path,
            ref child,
            status: ChildContextStatus::Done,
            ref result,
            ref refs_jsonl,
        })) if path == "home/1000/agent/base/session/default/context/plan.json"
            && child == "work-123"
            && result == "implemented"
            && refs_jsonl == "{\"id\":\"ref-abc\",\"path\":\"artifact/output.md\",\"kind\":\"artifact\",\"summary\":\"changed\"}\n"
    ));

    let cancelled = cmd!(
        "schedule",
        "result",
        "home/1000/agent/base/session/default/context/plan.json",
        "work-123",
        "cancelled",
        "interrupted",
    );
    assert!(matches!(
        cancelled,
        Ok(Command::Schedule(ScheduleArgs::Result {
            status: ChildContextStatus::Cancelled,
            ..
        }))
    ));

    let bad_status = cmd!(
        "schedule",
        "result",
        "home/1000/agent/base/session/default/context/plan.json",
        "work-123",
        "pending",
        "not terminal",
    );
    assert!(matches!(
        bad_status,
        Err(ref error)
            if error.code == 2
                && error.message == "schedule result status expects done, error, or cancelled"
    ));
}

#[test]
fn parses_ls_path_command() {
    let root = cmd!("ls");
    assert!(matches!(root, Ok(Command::Ls(LsTarget::Root))));

    let home = cmd!("ls", "home");
    assert!(matches!(
        home,
        Ok(Command::Ls(LsTarget::Path(ref path))) if path == "home"
    ));

    let tool = cmd!("ls", "tool");
    assert!(matches!(
        tool,
        Ok(Command::Ls(LsTarget::Path(ref path))) if path == "tool"
    ));
}
