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
fn model_event_frame_run_field_is_normalized_for_socket_runtime() {
    let frame = normalize_agent_model_frame(
        r#"{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#,
        "run-1",
    );

    assert!(frame.contains(r#""run":"run-1""#), "{frame}");
    assert!(frame.contains(r#""type":"tool_call""#), "{frame}");
    let frame = r#"{"type":"tool_call","run":"existing","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#;

    assert_eq!(normalize_agent_model_frame(frame, "run-1"), frame);
    assert_eq!(
        normalize_agent_model_frame("plain text", "run-1"),
        "plain text"
    );
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
fn tool_call_arguments_reject_excessive_limits() {
    let value = serde_json::json!({
        "type": "tool_call",
        "id": "call-1",
        "name": "tsh",
        "arguments": {
            "args": vec!["x"; 65]
        }
    });
    let call = agent_tool_call_from_value(&value);

    assert!(matches!(call, Err(ref error) if error.message().contains("argument count limit")));
    let value = serde_json::json!({
        "type": "tool_call",
        "id": "call-1",
        "name": "tsh",
        "arguments": {
            "command": format!("fs.read {}", "x".repeat(8 * 1024))
        }
    });
    let call = agent_tool_call_from_value(&value);

    assert!(matches!(call, Err(ref error) if error.message().contains("byte limit")));
}

#[test]
fn runtime_contract_describes_native_tsh_without_prompt_heuristics() {
    let contract = agent_runtime_contract("executor");

    for required in [
        "`tsh` is always native",
        "only tools statically declared by the agent `tools` control may also be direct-native",
        "Dynamically discovered, loaded, pinned, and cached tools stay `tsh`-only",
        "Do not claim provider, host, assistant-platform, or hidden-platform access, including `image_gen`",
        "request one native call and wait for its host result",
        r#"call `tsh` with `{"args":["..."]}`, where `args` is the exact `tsh` argv"#,
        "Results echo it with stdout/stderr or an ERROR line; use that output for the next action",
        "only when a user-requested file path is unknown; otherwise inspect to locate relevant files",
        "Before code changes, inspect; keep diffs small, write only needed files, and run focused verification",
        "Run `git reset --hard`, `git checkout --`, or `git clean` only on the user's exact request",
        "After results, continue the normal response",
        "interactive shells and tools, including bash, tmux, and zellij, only through `tsh`",
    ] {
        assert!(contract.contains(required), "missing {required}");
    }
    assert!(
        contract.contains("Never overwrite, revert, delete, or reformat unrelated user changes.")
    );
    for command in ["git reset --hard", "git checkout --", "git clean"] {
        assert!(contract.contains(command));
    }
}

#[test]
fn assistant_delta_text_is_not_treated_as_a_tool_call() {
    for text in [
        "我先通过 `tsh` 查看可用工具，再测试 lazy load 机制。",
        r#"{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#,
        r#"{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}后续说明。"#,
    ] {
        let frames = [serde_json::json!({ "type": "delta", "text": text }).to_string()];

        assert!(matches!(first_tool_call(&frames), Ok(None)), "{text}");
    }
}

#[test]
fn passthrough_tools_use_absolute_program_paths() {
    assert_eq!(passthrough_tool_program("bash"), Some("/usr/bin/bash"));
    assert_eq!(passthrough_tool_program("tmux"), Some("/usr/bin/tmux"));
    assert_eq!(passthrough_tool_program("zellij"), Some("/usr/bin/zellij"));
    assert_eq!(passthrough_tool_program("tsh"), Some("/usr/bin/tsh"));
    assert_eq!(passthrough_tool_program("shell.exec"), None);
    assert_eq!(passthrough_tool_program("fs.read"), None);
    assert!(!is_passthrough_tool("shell.exec"));
    assert!(!is_passthrough_tool("fs.read"));

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
            .arg(
                "object::executor::tests::parsing::passthrough_tool_gets_clean_runtime_environment",
            )
            .arg("--nocapture")
            .env("CORTEXFS_PASSTHROUGH_ENV_CHILD", "1")
            .env("CORTEXFS_SHOULD_NOT_LEAK", "secret")
            .env("CTX_AGENT", "executor")
            .output();
        assert!(matches!(output, Ok(ref output)
        if output.status.success()
            && String::from_utf8_lossy(&output.stdout).contains(
                "object::executor::tests::parsing::passthrough_tool_gets_clean_runtime_environment",
            )));
        return;
    }

    let result = run_passthrough_tool(
        "bash",
        &[
            OsString::from("-c"),
            OsString::from(
                r#"[ -z "$CORTEXFS_SHOULD_NOT_LEAK" ] && [ "$PATH" = "/usr/bin:/bin" ] && [ "$CTX_AGENT" = "executor" ]"#,
            ),
        ],
    );

    assert!(
        result.is_ok(),
        "passthrough environment was not clean: {result:?}"
    );
}
use super::*;
