#[test]
fn runner_recognizes_interactive_tool_passthroughs() {
    assert!(is_passthrough_tool("bash"));
    assert!(is_passthrough_tool("tmux"));
    assert!(is_passthrough_tool("zellij"));
    assert!(is_passthrough_tool("tsh"));
    assert!(!is_passthrough_tool("shell.exec"));
    assert!(!is_passthrough_tool("fs.read"));
}
#[test]
fn tool_call_text_parses_tsh_argv() {
    let call = tool_call_from_text(
        r#"{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#,
    );

    assert!(matches!(
        call,
        Ok(Some(ref call))
            if call.id == "call-1"
                && call.name == "tsh"
                && call.args == [OsString::from("tools")]
    ));
}

#[test]
fn tool_call_text_parses_json_prefix_before_prose() {
    let call = tool_call_from_text(
        r#"{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["fs.read","tmp/agent_edit_smoke.txt"]}}我没有拿到 `tsh` 工具执行结果。"#,
    );

    assert!(matches!(
        call,
        Ok(Some(ref call))
            if call.id == "call-1"
                && call.name == "tsh"
                && call.args
                    == [
                        OsString::from("fs.read"),
                        OsString::from("tmp/agent_edit_smoke.txt")
                    ]
    ));
}

#[test]
fn tool_call_arguments_accept_command_string() {
    let value = serde_json::json!({
        "type": "tool_call",
        "id": "call-1",
        "name": "tsh",
        "arguments": {
            "command": "fs.read README.md"
        }
    });
    let call = agent_tool_call_from_value(&value);

    assert!(matches!(
        call,
        Ok(Some(ref call))
            if call.args == [OsString::from("fs.read"), OsString::from("README.md")]
    ));
}

#[test]
fn tool_call_arguments_reject_excessive_arg_count() {
    let value = serde_json::json!({
        "type": "tool_call",
        "id": "call-1",
        "name": "tsh",
        "arguments": {
            "args": vec!["x"; 65]
        }
    });
    let call = agent_tool_call_from_value(&value);

    assert!(matches!(call, Err(ref error) if error.contains("argument count limit")));
}

#[test]
fn tool_call_arguments_reject_excessive_arg_bytes() {
    let value = serde_json::json!({
        "type": "tool_call",
        "id": "call-1",
        "name": "tsh",
        "arguments": {
            "command": format!("fs.read {}", "x".repeat(8 * 1024))
        }
    });
    let call = agent_tool_call_from_value(&value);

    assert!(matches!(call, Err(ref error) if error.contains("byte limit")));
}

#[test]
fn runtime_contract_requires_immediate_tsh_tool_calls() {
    let contract = agent_runtime_contract("coder");

    assert!(contract.contains("you must call `tsh` immediately"));
    assert!(contract.contains("Do not ask the user to let you execute `tsh`"));
    assert!(contract.contains("Do not say that you cannot execute `tsh`"));
    assert!(contract.contains("hidden platform tools such as `image_gen`"));
    assert!(contract.contains("output exactly one JSON object line and no prose"));
    assert!(contract.contains(r#""name":"tsh""#));
    assert!(contract.contains("If no concrete file path is provided"));
}

#[test]
fn deferred_tool_text_is_not_treated_as_a_tool_call() {
    let frames = [r#"{"type":"delta","text":"我先通过 `tsh` 查看可用工具，再测试 lazy load 机制。"}"#.to_owned()];

    assert!(matches!(first_tool_call(&frames), Ok(None)));
}

#[test]
fn reasoning_delta_text_is_not_treated_as_a_tool_call() {
    let frames = [
        r#"{"type":"reasoning_delta","text":"{\"type\":\"tool_call\",\"id\":\"call-1\",\"name\":\"tsh\",\"arguments\":{\"args\":[\"tools\"]}}"}"#.to_owned(),
        r#"{"type":"delta","text":"done"}"#.to_owned(),
    ];

    assert!(matches!(first_tool_call(&frames), Ok(None)));
}

#[test]
fn passthrough_tools_use_absolute_program_paths() {
    assert_eq!(passthrough_tool_program("bash"), Some("/usr/bin/bash"));
    assert_eq!(passthrough_tool_program("tmux"), Some("/usr/bin/tmux"));
    assert_eq!(passthrough_tool_program("zellij"), Some("/usr/bin/zellij"));
    assert_eq!(passthrough_tool_program("tsh"), Some("/usr/bin/tsh"));
    assert_eq!(passthrough_tool_program("shell.exec"), None);

    for (name, program) in [
        ("bash", "/usr/bin/bash"),
        ("tmux", "/usr/bin/tmux"),
        ("zellij", "/usr/bin/zellij"),
        ("tsh", "/usr/bin/tsh"),
    ] {
        assert!(Path::new(program).is_absolute());
        assert_eq!(passthrough_tool_program(name), Some(program));
        assert!(is_passthrough_tool(name));
    }
}

#[test]
fn passthrough_tool_gets_clean_runtime_environment() {
    if std::env::var_os("CORTEXFS_PASSTHROUGH_ENV_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap_or_default())
            .arg("--exact")
            .arg("tests::passthrough_tool_gets_clean_runtime_environment")
            .arg("--nocapture")
            .env("CORTEXFS_PASSTHROUGH_ENV_CHILD", "1")
            .env("CORTEXFS_SHOULD_NOT_LEAK", "secret")
            .env("CTX_AGENT", "coder")
            .output();
        assert!(matches!(output, Ok(ref output) if output.status.success()));
        return;
    }

    let result = run_passthrough_tool(
        "bash",
        &[
            OsString::from("-c"),
            OsString::from(
                r#"[ -z "$CORTEXFS_SHOULD_NOT_LEAK" ] && [ "$PATH" = "/usr/bin:/bin" ] && [ "$CTX_AGENT" = "coder" ]"#,
            ),
        ],
    );

    assert!(result.is_ok(), "passthrough environment was not clean: {result:?}");
}
