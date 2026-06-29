use super::{
    AgentToolCall, ObjectPath, OpenAiStreamEvent, ProviderRoute, ProviderRuntimeDriver,
    ResolvedTransport, RunnerProviderConfig, TokenUsage, agent_tool_call_from_value,
    execute_agent_tool_call, is_passthrough_tool, open_executable_no_follow,
    passthrough_tool_program,
    run_agent_model_once, run_agent_model_once_with_timeout, missing_model_message,
    is_regular_file_no_follow, model_candidates, openai_chat_body, openai_responses_body,
    openai_stream_event, parse_anthropic_message_content, parse_openai_response_content,
    curl_config_quote, open_runner_provider_config_dir, provider_config_from_dir,
    provider_curl_command, provider_messages_for_agent, provider_request_failure_message,
    provider_route, provider_runtime_driver,
    provider_secret_from_inherited_fd_with_env, provider_secret_from_runtime_file_with_env,
    provider_transport, read_provider_stream_line, read_runner_provider_config_file,
    read_runtime_provider_secret_file, read_small_plain_text_file, resolve_model_alias,
    resolved_model_path, run, read_runner_stdin_limited, run_agent_tool_loop,
    run_agent_tool_process_with_timeout, run_cli_tool_to_writer, run_passthrough_tool,
    token_usage_from_value, tool_call_from_text, trim_tool_result, validate_agent_tsh_args,
    write_model_text_or_tool_call, AgentModelRunConfig, AgentModelRunOutcome,
    agent_model_timeout_seconds_from_env, agent_tool_timeout_seconds_from_env,
    wait_for_curl_json_output, OpenAiStreamTextEmitter, MAX_AGENT_MODEL_FRAME_BYTES,
    MAX_AGENT_MODEL_FRAMES, MAX_AGENT_TOOL_CONTEXT_BYTES, MAX_AGENT_TOOL_OUTPUT_BYTES,
    MAX_CHILD_STDERR_BYTES, MAX_PROVIDER_RESPONSE_BYTES, MAX_PROVIDER_STREAM_LINE_BYTES,
    MAX_RUNNER_STDIN_INPUT_BYTES, MAX_STREAM_TOOL_CALL_BUFFER_BYTES, MAX_TOOL_RESULT_CHARS,
    PROVIDER_CURL_BIN, collect_child_stderr, first_tool_call, spawn_child_stderr_reader,
    split_object_args,
};
use cortexfs::{
    AgentPromptContext, DEFAULT_AGENT_PROMPT_TEMPLATE, agent_runtime_contract, collect_agent_rules,
    collect_skill_metadata, render_agent_system_prompt,
};
use std::ffi::OsString;
use std::fs;
use std::io::{Cursor, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(unix)]
fn unique_temp_dir(name: &str) -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "cortexfs-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir_all(&path)?;
    Ok(path)
}

#[cfg(unix)]
fn write_executable_script(path: &Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(content.as_ref())?;
    file.sync_all()?;
    drop(file);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(unix)]
fn short_unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cfs-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

#[test]
fn object_path_parses_class_and_name() {
    assert_eq!(
        ObjectPath::parse(Path::new("/ctx/model/debug/echo")),
        Ok(ObjectPath {
            class: "model".to_owned(),
            name: "debug/echo".to_owned(),
        })
    );
}

#[test]
fn object_args_use_exec_metadata_for_proc_fd_wrappers(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-object-path-proc-fd")?;
    let wrapper = root.join("coder");
    fs::write(
        &wrapper,
        "#!/usr/bin/cortexfs-object-runner\n# cortexfs.object=agent\n# cortexfs.name=coder\n",
    )?;
    let file = fs::File::open(&wrapper)?;
    let fd_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));

    let (path, rest) = split_object_args(vec![fd_path.into_os_string(), OsString::from("hello")])?;

    assert_eq!(path, PathBuf::from("/ctx/agent/coder"));
    assert_eq!(rest, vec![OsString::from("hello")]);
    assert_eq!(
        ObjectPath::parse(&path),
        Ok(ObjectPath {
            class: "agent".to_owned(),
            name: "coder".to_owned(),
        })
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}


#[test]
fn object_args_reject_exec_metadata_that_disagrees_with_authorized_object(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("CORTEXFS_RUNNER_METADATA_MISMATCH_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("cortexfs_object_runner_tests::object_args_reject_exec_metadata_that_disagrees_with_authorized_object")
            .arg("--nocapture")
            .env("CORTEXFS_RUNNER_METADATA_MISMATCH_CHILD", "1")
            .env("CTX_AUTHORIZED_OBJECT", "/ctx/tool/safe")
            .output()?;
        assert!(
            output.status.success(),
            "child test failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }

    let root = unique_temp_dir("runner-object-path-mismatch")?;
    let wrapper = root.join("safe");
    fs::write(
        &wrapper,
        "#!/usr/bin/cortexfs-object-runner\n# cortexfs.object=tool\n# cortexfs.name=bash\n",
    )?;
    let file = fs::File::open(&wrapper)?;
    let fd_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));

    let result = split_object_args(vec![fd_path.into_os_string(), OsString::from("hello")]);

    assert!(matches!(
        result,
        Err(error)
            if error.contains("/ctx/tool/bash")
                && error.contains("/ctx/tool/safe")
                && error.contains("does not match authorized object")
    ));

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn runner_rejects_missing_object_path() {
    assert_eq!(run(Vec::new()), Err("missing object path".to_owned()));
}

#[test]
fn runner_rejects_unknown_model() {
    assert_eq!(
        run(vec![OsString::from("/ctx/model/missing-provider/gpt-5.4")]),
        Err("missing provider: missing-provider".to_owned())
    );
}

#[test]
fn model_alias_resolves_only_ctx_model_objects() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runner-model-alias-ok-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("model"))?;
    symlink("/ctx/model/debug/echo", root.join("model/main"))?;

    assert_eq!(
        resolve_model_alias(&root, "main"),
        Ok("debug/echo".to_owned())
    );
    assert_eq!(
        resolved_model_path(&root, "main"),
        Ok(root.join("model/debug/echo"))
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_alias_rejects_cross_class_symlink_target() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runner-model-alias-bad-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("model"))?;
    symlink("../tool/shell.exec", root.join("model/main"))?;

    assert_eq!(
        resolve_model_alias(&root, "main"),
        Err("invalid model alias target: main".to_owned())
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_alias_rejects_symlink_model_directory() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-alias-symlink-model-dir")?;
    let outside = unique_temp_dir("runner-model-alias-symlink-model-dir-outside")?;
    fs::create_dir_all(&outside)?;
    symlink("/ctx/model/debug/echo", outside.join("main"))?;
    symlink(&outside, root.join("model"))?;

    assert_eq!(
        resolve_model_alias(&root, "main"),
        Err("missing model alias: main".to_owned())
    );
    assert_eq!(
        missing_model_message(&root, "main", &root.join("model/debug/echo")),
        format!("missing model: {}", root.join("model/debug/echo").display())
    );

    let _ignored = fs::remove_dir_all(root);
    let _ignored = fs::remove_dir_all(outside);
    Ok(())
}

#[test]
fn missing_model_message_names_dangling_alias_target() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-missing-model-message")?;
    fs::create_dir_all(root.join("model"))?;
    symlink("/ctx/model/localhost/gpt-5.4-mini", root.join("model/main"))?;

    assert_eq!(
        missing_model_message(&root, "main", &root.join("model/localhost/gpt-5.4-mini")),
        "missing model: main -> /ctx/model/localhost/gpt-5.4-mini"
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_path_rejects_traversal_reference() {
    let root = std::env::temp_dir().join("cortexfs-runner-model-path");

    assert_eq!(
        resolved_model_path(&root, "../tool/shell.exec"),
        Err("invalid model reference: ../tool/shell.exec".to_owned())
    );
}

#[test]
fn model_path_rejects_symlink_intermediate_directory(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-path-symlink-intermediate")?;
    let outside = unique_temp_dir("runner-model-path-symlink-intermediate-outside")?;
    fs::create_dir_all(outside.join("debug"))?;
    fs::write(outside.join("debug/echo"), "#!/bin/sh\n")?;
    symlink(&outside, root.join("model"))?;

    assert!(!is_regular_file_no_follow(&root.join("model/debug/echo")));

    let _ignored = fs::remove_dir_all(root);
    let _ignored = fs::remove_dir_all(outside);
    Ok(())
}

#[test]
fn model_candidates_follow_fallback_control_file() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-fallback")?;
    fs::create_dir_all(root.join("model/openai/gpt-5.5.d"))?;
    fs::write(
        root.join("model/openai/gpt-5.5.d/fallback"),
        "openai/codex-auto-review\nopenai/gpt-5.3-codex-spark\n",
    )?;

    let candidates = model_candidates(&root, "openai/gpt-5.5")?;

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        [
            "openai/gpt-5.5",
            "openai/codex-auto-review",
            "openai/gpt-5.3-codex-spark"
        ]
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_candidates_append_existing_local_counterpart(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-local-fallback")?;
    fs::create_dir_all(root.join("model/api.lmm.best/gpt-5.4-mini.d"))?;
    fs::write(root.join("model/api.lmm.best/gpt-5.4-mini"), "#!/bin/sh\n")?;
    fs::create_dir_all(root.join("model/local"))?;
    fs::write(root.join("model/local/gpt-5.4-mini"), "#!/bin/sh\n")?;

    let candidates = model_candidates(&root, "api.lmm.best/gpt-5.4-mini")?;

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        ["api.lmm.best/gpt-5.4-mini", "local/gpt-5.4-mini"]
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_candidates_ignore_symlinked_fallback_control_file(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-fallback-symlink")?;
    fs::create_dir_all(root.join("model/openai/gpt-5.5.d"))?;
    let outside = root.join("outside-fallback");
    fs::write(&outside, "openai/codex-auto-review\n")?;
    symlink(&outside, root.join("model/openai/gpt-5.5.d/fallback"))?;

    let candidates = model_candidates(&root, "openai/gpt-5.5")?;

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        ["openai/gpt-5.5"]
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn runner_control_reader_refuses_symlink() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-control-reader-symlink")?;
    let outside = root.join("outside-control");
    let link = root.join("control-link");
    fs::write(&outside, "secret\n")?;
    symlink(&outside, &link)?;

    let result = read_small_plain_text_file(&link);

    assert!(result.is_err());
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn runner_control_reader_refuses_symlink_intermediate_dir(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-control-reader-symlink-intermediate")?;
    let outside = root.join("outside");
    fs::create_dir_all(outside.join("control.d"))?;
    fs::write(outside.join("control.d/policy"), "secret\n")?;
    symlink(&outside, root.join("model"))?;

    let result = read_small_plain_text_file(&root.join("model/control.d/policy"));

    assert!(result.is_err());
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

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

#[test]
fn agent_tool_loop_supports_multiple_distinct_tsh_calls() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => {
                    assert!(config.tool_context.is_empty());
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"start","run":"r1","model":"main"}"#.to_owned(),
                            r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                            r#"{"type":"done","run":"r1"}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                2 => {
                    assert!(config.tool_context.contains("Tool result call-1 from tsh"));
                    assert!(config.tool_context.contains("fs.read"));
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"tool_call","run":"r1","id":"call-2","name":"tsh","arguments":{"args":["load","fs.read"]}}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                3 => {
                    assert!(config.tool_context.contains("Tool result call-2 from tsh"));
                    assert!(config.tool_context.contains("loaded fs.read"));
                    Ok(AgentModelRunOutcome {
                        frames: vec![r#"{"type":"delta","run":"r1","text":"ready"}"#.to_owned()],
                        success: true,
                        streamed: false,
                    })
                }
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            let args = tool_call
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            executed.push(args.clone());
            if args.first().is_some_and(|arg| arg == "tools") && args.len() == 1 {
                return Ok("fs.read\ntsh\n".to_owned());
            }
            if args.first().is_some_and(|arg| arg == "load")
                && args.get(1).is_some_and(|arg| arg == "fs.read")
                && args.len() == 2
            {
                return Ok("loaded fs.read\t/ctx/tool/fs.read\tmetadata\n".to_owned());
            }
            Ok(format!("unexpected args: {args:?}"))
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(
        executed,
        vec![
            vec!["tools".to_owned()],
            vec!["load".to_owned(), "fs.read".to_owned()]
        ]
    );
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""id":"call-1""#));
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains(r#""id":"call-2""#));
    assert!(output.contains(r#""tool_call_id":"call-2""#));
    assert!(
        output.contains(r#""text":"ready""#),
        "missing ready frame in output:\n{output}\ncontext:\n{}",
        config.tool_context
    );
}

#[test]
fn agent_tool_loop_executes_initial_tool_discovery_request() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "请立刻调用 tsh tools 探索你有哪些工具",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            assert_eq!(step, 1);
            assert!(config.tool_context.contains("Tool result call-1 from tsh"));
            assert!(config.tool_context.contains("fs.read"));
            Ok(AgentModelRunOutcome {
                frames: vec![r#"{"type":"delta","run":"r1","text":"有 fs.read"}"#.to_owned()],
                success: true,
                streamed: false,
            })
        },
        |_config, tool_call| {
            executed.push(
                tool_call
                    .args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
            );
            Ok("fs.read\nfs.write\ntsh\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(executed, vec![vec!["tools".to_owned()]]);
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""id":"call-1""#));
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains(r#""text":"有 fs.read""#));
}

#[test]
fn agent_tool_loop_executes_explicit_backtick_tsh_sequence() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "执行 `tsh fs.read /workspace/smoke.txt` 然后 `tsh fs.write /workspace/smoke.txt status=done`",
        &mut output,
        |config, _input, _stdout| {
            assert!(config.tool_context.contains("Tool result call-1 from tsh"));
            assert!(config.tool_context.contains("Tool result call-2 from tsh"));
            Ok(AgentModelRunOutcome {
                frames: vec![r#"{"type":"delta","run":"r1","text":"done"}"#.to_owned()],
                success: true,
                streamed: false,
            })
        },
        |_config, tool_call| {
            let args = tool_call
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            executed.push(args.clone());
            Ok(format!("executed {args:?}\n"))
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(
        executed,
        vec![
            vec!["fs.read".to_owned(), "/workspace/smoke.txt".to_owned()],
            vec![
                "fs.write".to_owned(),
                "/workspace/smoke.txt".to_owned(),
                "status=done".to_owned()
            ]
        ]
    );
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains(r#""tool_call_id":"call-2""#));
    assert!(output.contains(r#""text":"done""#));
}

#[test]
fn agent_tool_loop_falls_back_to_tool_result_when_followup_model_fails() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => {
                    assert!(!config.suppress_model_error_events);
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                2 => {
                    assert!(config.suppress_model_error_events);
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"error","run":"r1","code":"EIO","message":"model failed after tool result"}"#.to_owned(),
                        ],
                        success: false,
                        streamed: false,
                    })
                }
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            assert_eq!(tool_call.name, "tsh");
            Ok("fs.read\nshell.exec\ntsh\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains("fs.read"));
    assert!(output.contains("shell.exec"));
    assert!(output.contains("工具 `tsh` 已执行"));
    assert!(output.contains(r#""status":"ok""#));
    assert!(!output.contains(r#""type":"error""#), "{output}");
}

#[test]
fn agent_tool_loop_falls_back_when_followup_has_no_visible_reply() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => Ok(AgentModelRunOutcome {
                    frames: vec![
                        r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                    ],
                    success: true,
                    streamed: false,
                }),
                2 => {
                    assert!(config.suppress_model_error_events);
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"usage","run":"r1","input_tokens":10,"output_tokens":0}"#.to_owned(),
                            r#"{"type":"done","run":"r1","status":"ok"}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            assert_eq!(tool_call.name, "tsh");
            Ok("fs.read\nfs.write\ntsh\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains("fs.read"));
    assert!(output.contains("fs.write"));
    assert!(output.contains("工具 `tsh` 已执行"));
    assert!(output.contains(r#""status":"ok""#));
}

#[test]
fn agent_tool_loop_wraps_followup_plain_text_as_event() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |_config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => Ok(AgentModelRunOutcome {
                    frames: vec![
                        r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                    ],
                    success: true,
                    streamed: false,
                }),
                2 => Ok(AgentModelRunOutcome {
                    frames: vec!["工具已经列出。".to_owned()],
                    success: true,
                    streamed: false,
                }),
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, _tool_call| Ok("fs.read\ntsh\n".to_owned()),
    );

    assert_eq!(result, Ok(()));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""type":"delta""#), "{output}");
    assert!(output.contains("工具已经列出。"), "{output}");
}

#[test]
fn agent_tool_loop_falls_back_on_repeated_identical_tsh_call() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;
    let mut executions = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "repeat tools",
        &mut output,
        |_config, _input, _stdout| {
            step = step.saturating_add(1);
            assert!(step <= 2);
            Ok(AgentModelRunOutcome {
                frames: vec![
                    r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                ],
                success: true,
                streamed: false,
            })
        },
        |_config, _tool_call| {
            executions = executions.saturating_add(1);
            Ok("fs.read\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(executions, 1);
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""status":"ok""#));
    assert!(output.contains("工具 `tsh` 已执行"));
    assert!(output.contains("fs.read"));
    assert!(!output.contains(r#""code":"ELOOP""#), "{output}");
}

#[test]
fn agent_tool_loop_passes_tool_context_to_model_process_across_iterations()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-context")?;
    let model = root.join("model-script");
    write_executable_script(
        &model,
        r#"#!/bin/sh
case "$CTX_AGENT_TOOL_CONTEXT" in
  "")
    printf '{"type":"tool_call","run":"%s","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}\n' "$CTX_RUN_ID"
    ;;
  *"Tool result call-2 from tsh"*"loaded fs.read"*)
    printf '{"type":"delta","run":"%s","text":"ready"}\n' "$CTX_RUN_ID"
    printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
    ;;
  *"Tool result call-1 from tsh"*fs.read*)
    printf '{"type":"tool_call","run":"%s","id":"call-2","name":"tsh","arguments":{"args":["load","fs.read"]}}\n' "$CTX_RUN_ID"
    ;;
  *)
    printf '{"type":"error","run":"%s","code":"EIO","message":"missing tool context"}\n' "$CTX_RUN_ID"
    exit 2
    ;;
esac
"#,
    )?;
    let mut config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        run_agent_model_once,
        |_config, tool_call| {
            let args = tool_call
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            executed.push(args.clone());
            if args.first().is_some_and(|arg| arg == "tools") && args.len() == 1 {
                return Ok("fs.read\ntsh\n".to_owned());
            }
            if args.first().is_some_and(|arg| arg == "load")
                && args.get(1).is_some_and(|arg| arg == "fs.read")
                && args.len() == 2
            {
                return Ok("loaded fs.read\t/ctx/tool/fs.read\tmetadata\n".to_owned());
            }
            Err(format!("unexpected args: {args:?}"))
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(
        executed,
        vec![
            vec!["tools".to_owned()],
            vec!["load".to_owned(), "fs.read".to_owned()]
        ]
    );
    assert!(config.tool_context.contains("Tool result call-1 from tsh"));
    assert!(config.tool_context.contains("Tool result call-2 from tsh"));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""id":"call-1""#));
    assert!(output.contains(r#""id":"call-2""#));
    assert!(
        output.contains(r#""text":"ready""#),
        "missing ready frame in output:\n{output}\ncontext:\n{}",
        config.tool_context
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_tool_context_keeps_recent_results_under_limit() {
    let mut config = test_agent_run_config();

    for index in 0..12 {
        let call = AgentToolCall {
            id: format!("call-{index}"),
            name: "tsh".to_owned(),
            args: Vec::new(),
        };
        config.push_tool_result(&call, &"x".repeat(8 * 1024));
    }

    assert!(config.tool_context.len() <= MAX_AGENT_TOOL_CONTEXT_BYTES);
    assert!(config.tool_context.contains("[earlier tool context truncated]"));
    assert!(config.tool_context.contains("Tool result call-11 from tsh"));
    assert!(!config.tool_context.contains("Tool result call-0 from tsh"));
}

#[test]
fn agent_tool_context_truncation_preserves_utf8_boundaries() {
    let mut config = test_agent_run_config();
    config.tool_context = "€".repeat((MAX_AGENT_TOOL_CONTEXT_BYTES / "€".len()) + 64);
    let call = AgentToolCall {
        id: "call-final".to_owned(),
        name: "tsh".to_owned(),
        args: Vec::new(),
    };

    config.push_tool_result(&call, "done\n");

    assert!(config.tool_context.len() <= MAX_AGENT_TOOL_CONTEXT_BYTES);
    assert!(config.tool_context.is_char_boundary(config.tool_context.len()));
    assert!(config.tool_context.contains("Tool result call-final from tsh"));
}

#[test]
fn agent_model_process_gets_clean_runtime_environment() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("CORTEXFS_ENV_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("tests::agent_model_process_gets_clean_runtime_environment")
            .arg("--nocapture")
            .env("CORTEXFS_ENV_CHILD", "1")
            .env("CORTEXFS_SHOULD_NOT_LEAK", "secret")
            .env("CTX_PROVIDER_SECRET_PROVIDER", "local")
            .env("CTX_PROVIDER_SECRET_SLOT", "default")
            .env("CTX_PROVIDER_SECRET_PATH", "/tmp/runtime-secret")
            .output()?;
        assert!(
            output.status.success(),
            "child test failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }

    let root = unique_temp_dir("runner-agent-model-env")?;
    let model = root.join("model-script");
    write_executable_script(
        &model,
        r#"#!/bin/sh
if [ -n "$CORTEXFS_SHOULD_NOT_LEAK" ]; then
  printf '{"type":"error","run":"%s","code":"EIO","message":"inherited parent env"}\n' "$CTX_RUN_ID"
  exit 2
fi
if [ "$PATH" != "/usr/bin:/bin" ]; then
  printf '{"type":"error","run":"%s","code":"EIO","message":"unexpected path"}\n' "$CTX_RUN_ID"
  exit 2
fi
if [ "$CTX_PROVIDER_SECRET_PROVIDER" != "local" ] || [ "$CTX_PROVIDER_SECRET_SLOT" != "default" ] || [ "$CTX_PROVIDER_SECRET_PATH" != "/tmp/runtime-secret" ]; then
  printf '{"type":"error","run":"%s","code":"EIO","message":"missing runtime provider secret env"}\n' "$CTX_RUN_ID"
  exit 2
fi
printf '{"type":"delta","run":"%s","text":"clean"}\n' "$CTX_RUN_ID"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    )?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();

    let result = run_agent_model_once(&config, "hello", &mut output);

    let outcome = result?;
    assert!(outcome.success);
    assert!(outcome.frames.iter().any(|frame| frame.contains(r#""text":"clean""#)));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_model_process_rejects_symlink_executable_without_running_target()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-symlink-executable")?;
    let target = root.join("target-script");
    let model = root.join("model-script");
    let marker = root.join("target-ran");
    fs::write(
        &target,
        format!(
            "#!/bin/sh\nprintf ran > '{}'\nprintf '{{\"type\":\"done\",\"run\":\"%s\",\"status\":\"ok\"}}\\n' \"$CTX_RUN_ID\"\n",
            marker.display()
        ),
    )?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
    symlink(&target, &model)?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();

    let result = run_agent_model_once(&config, "hello", &mut output);

    assert!(result.is_err());
    assert!(!marker.exists(), "symlink target was executed");
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_model_process_timeout_kills_process_group() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-timeout")?;
    let model = root.join("model-script");
    let marker = root.join("child-survived");
    write_executable_script(
        &model,
        format!(
            "#!/bin/sh\n(trap '' TERM; sleep 1; printf alive > '{}') &\nsleep 5\n",
            marker.display()
        ),
    )?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();
    let started = Instant::now();

    let outcome =
        run_agent_model_once_with_timeout(&config, "hello", &mut output, Duration::from_millis(100))?;

    assert!(!outcome.success);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(outcome.frames.iter().any(|frame| frame.contains(r#""code":"ETIMEDOUT""#)));
    thread::sleep(Duration::from_millis(1500));
    assert!(
        !marker.exists(),
        "model timeout killed only the parent process; child process survived"
    );
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""code":"ETIMEDOUT""#));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_model_output_rejects_oversized_frame() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-oversized-frame")?;
    let model = root.join("model-script");
    write_executable_script(
        &model,
        format!(
            "#!/bin/sh\nhead -c {MAX_AGENT_MODEL_FRAME_BYTES} /dev/zero | tr '\\0' x\nprintf '\\n'\n"
        ),
    )?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();

    let outcome = run_agent_model_once(&config, "hello", &mut output)?;

    assert!(!outcome.success);
    assert!(outcome.frames.iter().any(|frame| frame.contains(r#""code":"EOVERFLOW""#)));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""code":"EOVERFLOW""#));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_model_output_rejects_too_many_frames() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-too-many-frames")?;
    let model = root.join("model-script");
    write_executable_script(
        &model,
        format!(
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -le {MAX_AGENT_MODEL_FRAMES} ]; do printf '{{\"type\":\"delta\",\"run\":\"%s\",\"text\":\"x\"}}\\n' \"$CTX_RUN_ID\"; i=$((i + 1)); done\n"
        ),
    )?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();

    let outcome = run_agent_model_once(&config, "hello", &mut output)?;

    assert!(!outcome.success);
    assert!(outcome.frames.iter().any(|frame| frame.contains(r#""code":"EOVERFLOW""#)));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""code":"EOVERFLOW""#));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_tool_call_executes_visible_tsh_for_search_and_load() -> Result<(), Box<dyn std::error::Error>>
{
    let root = short_unique_temp_path("atl");
    let _ignored = fs::remove_dir_all(&root);
    let control = root.join("agent").join("coder.d");
    let tool_control = root.join("tool").join("tsh.d");
    fs::create_dir_all(&control)?;
    fs::create_dir_all(&tool_control)?;
    fs::create_dir_all(root.join("tool"))?;
    fs::write(control.join("owner"), "1000\n")?;
    fs::write(control.join("uid"), "1000\n")?;
    fs::write(control.join("gid"), "1000\n")?;
    fs::write(control.join("groups"), "1000\n")?;
    fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n")?;
    fs::write(control.join("iso"), "shared\n")?;
    fs::write(control.join("parent"), "\n")?;
    fs::write(control.join("life"), "owned\n")?;
    fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n")?;
    fs::write(control.join("cwd"), "/workspace\n")?;
    fs::write(control.join("env"), "\n")?;
    fs::write(control.join("model"), "main\n")?;
    fs::write(control.join("status"), "idle\n")?;
    fs::write(control.join("pid"), "\n")?;
    fs::write(control.join("log"), "\n")?;
    fs::write(control.join("meta.json"), "{}\n")?;
    fs::write(control.join("path"), format!("{}\n", root.join("tool").display()))?;
    fs::write(
        control.join("mount"),
        format!(
            "{}\t{}\tro\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    )?;
    fs::write(
        control.join("policy"),
        "allow coder_t model:main use\nallow coder_t tool:tsh execute\n",
    )?;
    fs::write(
        tool_control.join("policy"),
        "allow coder_t tool:tsh execute\n",
    )?;
    fs::write(
        root.join("tool").join("tsh"),
        r#"#!/bin/sh
case "$1" in
  tools)
    printf 'fs.read\nshell.exec\ntsh\n'
    ;;
  load)
    if [ "$2" = "fs.read" ]; then
      printf 'loaded fs.read\t%s/tool/fs.read\tmetadata\n' "$CTX_ROOT"
    else
      printf 'unknown tool: %s\n' "$2" >&2
      exit 2
    fi
    ;;
  *)
    printf 'unexpected tsh args: %s %s\n' "$1" "$2" >&2
    exit 2
    ;;
esac
"#,
    )?;
    fs::set_permissions(root.join("tool").join("tsh"), fs::Permissions::from_mode(0o755))?;
    let config = AgentModelRunConfig {
        ctx_root: root.clone(),
        source: root.clone(),
        ..test_agent_run_config()
    };

    let search = AgentToolCall {
        id: "call-1".to_owned(),
        name: "tsh".to_owned(),
        args: vec![OsString::from("tools")],
    };
    let search_result = execute_agent_tool_call(&config, &search)?;

    assert!(search_result.contains("fs.read"));
    assert!(search_result.contains("tsh"));

    let load = AgentToolCall {
        id: "call-2".to_owned(),
        name: "tsh".to_owned(),
        args: vec![OsString::from("load"), OsString::from("fs.read")],
    };
    let load_result = execute_agent_tool_call(&config, &load)?;

    assert!(load_result.contains("loaded fs.read"));
    assert!(load_result.contains("/tool/fs.read"));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_tool_call_refuses_symlinked_tsh_policy() -> Result<(), Box<dyn std::error::Error>> {
    let root = short_unique_temp_path("atl-policy-symlink");
    let _ignored = fs::remove_dir_all(&root);
    let control = root.join("agent").join("coder.d");
    let tool_control = root.join("tool").join("tsh.d");
    fs::create_dir_all(&control)?;
    fs::create_dir_all(&tool_control)?;
    fs::create_dir_all(root.join("tool"))?;
    fs::write(control.join("owner"), "1000\n")?;
    fs::write(control.join("uid"), "1000\n")?;
    fs::write(control.join("gid"), "1000\n")?;
    fs::write(control.join("groups"), "1000\n")?;
    fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n")?;
    fs::write(control.join("iso"), "shared\n")?;
    fs::write(control.join("parent"), "\n")?;
    fs::write(control.join("life"), "owned\n")?;
    fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n")?;
    fs::write(control.join("cwd"), "/workspace\n")?;
    fs::write(control.join("env"), "\n")?;
    fs::write(control.join("model"), "main\n")?;
    fs::write(control.join("status"), "idle\n")?;
    fs::write(control.join("pid"), "\n")?;
    fs::write(control.join("log"), "\n")?;
    fs::write(control.join("meta.json"), "{}\n")?;
    fs::write(control.join("path"), format!("{}\n", root.join("tool").display()))?;
    fs::write(
        control.join("mount"),
        format!(
            "{}\t{}\tro\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    )?;
    fs::write(
        control.join("policy"),
        "allow coder_t model:main use\nallow coder_t tool:tsh execute\n",
    )?;
    let outside_policy = root.join("outside-policy");
    fs::write(&outside_policy, "allow coder_t tool:tsh execute\n")?;
    symlink(&outside_policy, tool_control.join("policy"))?;
    fs::write(root.join("tool").join("tsh"), "#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(root.join("tool").join("tsh"), fs::Permissions::from_mode(0o755))?;
    let config = AgentModelRunConfig {
        ctx_root: root.clone(),
        source: root.clone(),
        ..test_agent_run_config()
    };

    let call = AgentToolCall {
        id: "call-1".to_owned(),
        name: "tsh".to_owned(),
        args: vec![OsString::from("tools")],
    };
    let result = execute_agent_tool_call(&config, &call);

    assert!(matches!(result, Err(ref error) if error.contains("cannot read")));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn object_runner_executable_open_refuses_symlink_tool() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-executable-symlink")?;
    let target = root.join("target-tool");
    let link = root.join("tsh");
    fs::write(&target, "#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
    symlink(&target, &link)?;

    assert!(open_executable_no_follow(&link).is_err());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn async_stderr_reader_drains_large_child_stderr() -> Result<(), Box<dyn std::error::Error>> {
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg("i=0; while [ $i -lt 10000 ]; do printf 'stderr line %04d\\n' \"$i\" >&2; i=$((i + 1)); done")
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let stderr_reader = child.stderr.take().map(spawn_child_stderr_reader);
    let _status = child.wait()?;
    let stderr = collect_child_stderr(stderr_reader);

    assert!(stderr.len() <= MAX_CHILD_STDERR_BYTES);
    assert!(stderr.contains("stderr line"));
    Ok(())
}

#[test]
fn stream_tool_call_buffer_rejects_oversized_json_prefix() {
    let mut emitter = OpenAiStreamTextEmitter::new("run-1");
    let mut output = Vec::new();
    let oversized = format!(
        "{{\"type\":\"tool_call\",\"padding\":\"{}\"",
        "x".repeat(MAX_STREAM_TOOL_CALL_BUFFER_BYTES)
    );

    let result = emitter.push(&mut output, &oversized);

    assert!(result.is_err());
}

#[test]
fn runner_stdin_reader_accepts_input_at_limit() {
    let input = "x".repeat(MAX_RUNNER_STDIN_INPUT_BYTES);

    let read = read_runner_stdin_limited(Cursor::new(input.as_bytes()), MAX_RUNNER_STDIN_INPUT_BYTES);

    assert_eq!(read.unwrap_or_default().len(), MAX_RUNNER_STDIN_INPUT_BYTES);
}

#[test]
fn runner_stdin_reader_rejects_input_over_limit() {
    let input = "x".repeat(MAX_RUNNER_STDIN_INPUT_BYTES + 1);

    let read = read_runner_stdin_limited(Cursor::new(input.as_bytes()), MAX_RUNNER_STDIN_INPUT_BYTES);

    assert!(matches!(read, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
fn agent_tool_process_times_out_instead_of_hanging() {
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg("sleep 5");
    let started = Instant::now();

    let result = run_agent_tool_process_with_timeout(&mut command, Duration::from_millis(100));

    assert!(matches!(result, Err(ref error) if error.contains("timed out")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn agent_tool_process_returns_when_grandchild_keeps_stdout_open() {
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg("printf done; (sleep 5) & exit 0");
    let started = Instant::now();

    let output = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(2));

    assert!(output.is_ok());
    let Ok(output) = output else { return };
    assert!(output.status.success());
    assert_eq!(output.stdout, b"done");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn agent_tool_process_kills_child_after_oversized_output() {
    let mut command = std::process::Command::new("sh");
    let oversized = MAX_AGENT_TOOL_OUTPUT_BYTES + 1;
    command
        .arg("-c")
        .arg(format!("yes x | head -c {oversized}; sleep 5"));
    let started = Instant::now();

    let result = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(10));

    assert!(matches!(result, Err(ref error) if error.contains("tool output exceeds")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn agent_tool_process_rejects_fast_oversized_output() {
    let mut command = std::process::Command::new("sh");
    let oversized = MAX_AGENT_TOOL_OUTPUT_BYTES + 1;
    command.arg("-c").arg(format!("yes x | head -c {oversized}"));

    let result = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(2));

    assert!(matches!(result, Err(ref error) if error.contains("tool output exceeds")));
}

#[test]
fn agent_tool_process_accepts_output_at_limit() {
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg(format!("yes x | head -c {MAX_AGENT_TOOL_OUTPUT_BYTES}"));

    let output = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(2));

    assert!(output.is_ok());
    let Ok(output) = output else { return };
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), MAX_AGENT_TOOL_OUTPUT_BYTES);
}

#[test]
fn tool_result_truncation_preserves_utf8_boundaries() {
    let oversized = "€".repeat((16 * 1024 / "€".len()) + 1);

    let result = std::panic::catch_unwind(|| trim_tool_result(&oversized));

    assert!(result.is_ok());
    let trimmed = result.unwrap_or_default();
    assert!(trimmed.ends_with("\n[truncated]\n"));
    assert!(trimmed.len() <= MAX_TOOL_RESULT_CHARS);
    assert!(trimmed.is_char_boundary(trimmed.len()));
}

#[test]
fn agent_tool_timeout_env_is_bounded() {
    assert_eq!(
        agent_tool_timeout_seconds_from_env(|_| Some("45".to_owned())),
        45
    );
    assert_eq!(
        agent_tool_timeout_seconds_from_env(|_| Some("0".to_owned())),
        20
    );
    assert_eq!(
        agent_tool_timeout_seconds_from_env(|_| Some("999999".to_owned())),
        20
    );
    assert_eq!(
        agent_tool_timeout_seconds_from_env(|_| Some("bad".to_owned())),
        20
    );
    assert_eq!(agent_tool_timeout_seconds_from_env(|_| None), 20);
}

#[test]
fn agent_model_timeout_env_is_bounded() {
    assert_eq!(
        agent_model_timeout_seconds_from_env(|_| Some("45".to_owned())),
        45
    );
    assert_eq!(
        agent_model_timeout_seconds_from_env(|_| Some("0".to_owned())),
        120
    );
    assert_eq!(
        agent_model_timeout_seconds_from_env(|_| Some("999999".to_owned())),
        120
    );
    assert_eq!(
        agent_model_timeout_seconds_from_env(|_| Some("bad".to_owned())),
        120
    );
    assert_eq!(agent_model_timeout_seconds_from_env(|_| None), 120);
}

#[test]
fn agent_tsh_args_reject_root_override() {
    assert_eq!(
        validate_agent_tsh_args(&[
            OsString::from("--root"),
            OsString::from("/tmp/fakectx"),
            OsString::from("evil"),
        ]),
        Err("tool_call args cannot override tsh root".to_owned())
    );
    assert_eq!(
        validate_agent_tsh_args(&[
            OsString::from("-r"),
            OsString::from("/tmp/fakectx"),
            OsString::from("evil"),
        ]),
        Err("tool_call args cannot override tsh root".to_owned())
    );
}

#[test]
fn agent_tsh_args_reject_empty_args() {
    assert_eq!(
        validate_agent_tsh_args(&[]),
        Err("tool_call args for tsh cannot be empty".to_owned())
    );
}

#[test]
fn agent_tsh_args_reject_recursive_tsh_program_name() {
    assert_eq!(
        validate_agent_tsh_args(&[OsString::from("tsh")]),
        Err("tool_call args for tsh must not include the tsh program name".to_owned())
    );
    assert_eq!(
        validate_agent_tsh_args(&[OsString::from("tsh"), OsString::from("tools")]),
        Err("tool_call args for tsh must not include the tsh program name".to_owned())
    );
}

#[test]
fn agent_tsh_args_allow_tool_arguments_after_tool_name() {
    assert_eq!(
        validate_agent_tsh_args(&[
            OsString::from("fs.read"),
            OsString::from("--root"),
            OsString::from("README.md"),
        ]),
        Ok(())
    );
}

#[test]
fn execute_agent_tsh_call_rejects_empty_args() -> Result<(), Box<dyn std::error::Error>> {
    let root = short_unique_temp_path("atl-empty-args");
    let _ignored = fs::remove_dir_all(&root);
    let control = root.join("agent").join("coder.d");
    let tool_control = root.join("tool").join("tsh.d");
    fs::create_dir_all(&control)?;
    fs::create_dir_all(&tool_control)?;
    fs::create_dir_all(root.join("tool"))?;
    fs::write(control.join("owner"), "1000\n")?;
    fs::write(control.join("uid"), "1000\n")?;
    fs::write(control.join("gid"), "1000\n")?;
    fs::write(control.join("groups"), "1000\n")?;
    fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n")?;
    fs::write(control.join("iso"), "shared\n")?;
    fs::write(control.join("parent"), "\n")?;
    fs::write(control.join("life"), "owned\n")?;
    fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n")?;
    fs::write(control.join("cwd"), "/workspace\n")?;
    fs::write(control.join("env"), "\n")?;
    fs::write(control.join("model"), "main\n")?;
    fs::write(control.join("status"), "idle\n")?;
    fs::write(control.join("pid"), "\n")?;
    fs::write(control.join("log"), "\n")?;
    fs::write(control.join("meta.json"), "{}\n")?;
    fs::write(control.join("path"), format!("{}\n", root.join("tool").display()))?;
    fs::write(
        control.join("mount"),
        format!(
            "{}\t{}\tro\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    )?;
    fs::write(
        control.join("policy"),
        "allow coder_t model:main use\nallow coder_t tool:tsh execute\n",
    )?;
    fs::write(
        tool_control.join("policy"),
        "allow coder_t tool:tsh execute\n",
    )?;

    let executed = root.join("tsh-called");
    write_executable_script(
        &root.join("tool").join("tsh"),
        format!("#!/bin/sh\ntouch {}\n", executed.display()),
    )?;

    let config = AgentModelRunConfig {
        ctx_root: root.clone(),
        source: root.clone(),
        ..test_agent_run_config()
    };
    let call = AgentToolCall {
        id: "call-1".to_owned(),
        name: "tsh".to_owned(),
        args: Vec::new(),
    };
    let result = execute_agent_tool_call(&config, &call);

    assert_eq!(
        result,
        Err("tool_call args for tsh cannot be empty".to_owned())
    );
    assert!(!executed.exists());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_text_tool_call_writes_canonical_event() {
    let mut output = Vec::new();
    let result = write_model_text_or_tool_call(
        &mut output,
        "run-1",
        r#"{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#,
    );

    assert!(result.is_ok());
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""type":"tool_call""#));
    assert!(output.contains(r#""run":"run-1""#));
    assert!(output.contains(r#""name":"tsh""#));
    assert!(!output.contains(r#""type":"delta""#));
}

#[test]
fn cli_tool_mode_outputs_plain_text() {
    let path = std::env::temp_dir().join(format!("cortexfs-runner-cli-{}", std::process::id()));
    assert!(fs::write(&path, "plain").is_ok());
    let mut output = Vec::new();
    let result = run_cli_tool_to_writer("fs.read", &[OsString::from(&path)], &mut output);
    assert!(result.is_ok());
    assert_eq!(String::from_utf8(output).unwrap_or_default(), "plain");
    let _ignored = fs::remove_file(path);
}

#[test]
fn openai_stream_event_extracts_chat_delta_text() {
    let event = openai_stream_event(
        r#"data: {"choices":[{"delta":{"content":"hel"}}]}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text == "hel"));
}

#[test]
fn openai_stream_event_extracts_usage() {
    let event = openai_stream_event(
        r#"data: {"usage":{"prompt_tokens":12,"completion_tokens":5}}"#,
    );
    assert!(matches!(
        event,
        Ok(OpenAiStreamEvent::Usage(TokenUsage {
            input_tokens: 12,
            output_tokens: 5
        }))
    ));
}

#[test]
fn openai_stream_event_accepts_done_marker() {
    assert!(matches!(
        openai_stream_event("data: [DONE]"),
        Ok(OpenAiStreamEvent::Done)
    ));
}

#[test]
fn provider_stream_reader_rejects_oversized_line() {
    let input = format!("{}\n", "x".repeat(MAX_PROVIDER_STREAM_LINE_BYTES));
    let mut reader = Cursor::new(input.into_bytes());

    let result = read_provider_stream_line(&mut reader);

    assert!(result.is_err());
}

#[test]
fn openai_stream_event_extracts_responses_delta_text() {
    let event = openai_stream_event(
        r#"data: {"type":"response.output_text.delta","delta":"hel"}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text == "hel"));
}

#[test]
fn openai_stream_event_extracts_responses_done_text() {
    let event = openai_stream_event(
        r#"data: {"type":"response.output_text.done","text":"hel"}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::FinalText(text)) if text == "hel"));

    let event = openai_stream_event(
        r#"data: {"type":"response.content_part.done","part":{"type":"output_text","text":"lo"}}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::FinalText(text)) if text == "lo"));

    let event = openai_stream_event(
        r#"data: {"type":"response.output_item.done","item":{"content":[{"type":"output_text","text":"ok"}]}}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::FinalText(text)) if text == "ok"));
}

#[test]
fn responses_done_text_is_not_treated_as_stream_delta() -> Result<(), Box<dyn std::error::Error>> {
    let events = [
        openai_stream_event(r#"data: {"type":"response.output_text.delta","delta":"Hi!"}"#),
        openai_stream_event(r#"data: {"type":"response.output_text.done","text":"Hi!"}"#),
    ];
    let mut output = Vec::new();
    let mut emitter = OpenAiStreamTextEmitter::new("run-1");
    let mut has_delta = false;

    for event in events {
        match event? {
            OpenAiStreamEvent::Delta(text) if !text.is_empty() => {
                emitter.push(&mut output, &text)?;
                has_delta = true;
            }
            OpenAiStreamEvent::FinalText(text) if !has_delta && !text.is_empty() => {
                emitter.push(&mut output, &text)?;
                has_delta = true;
            }
            _ => {}
        }
    }
    emitter.finish(&mut output)?;

    let rendered = String::from_utf8(output)?;
    assert_eq!(rendered.matches("Hi!").count(), 1);
    Ok(())
}

#[test]
fn openai_stream_event_accepts_responses_completed_marker() {
    assert!(matches!(
        openai_stream_event(r#"data: {"type":"response.completed"}"#),
        Ok(OpenAiStreamEvent::Done)
    ));
}

#[test]
fn openai_stream_event_reports_responses_failed_marker() {
    assert_eq!(
        openai_stream_event(
            r#"data: {"type":"response.failed","error":{"message":"quota exceeded"}}"#
        )
        .map(|event| matches!(event, OpenAiStreamEvent::Ignore)),
        Err("quota exceeded".to_owned())
    );
}

#[test]
fn openai_stream_event_does_not_mix_reasoning_into_answer_text() {
    let event = openai_stream_event(
        r#"data: {"choices":[{"delta":{"reasoning_content":"hidden"}}]}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text.is_empty()));
}

#[test]
fn openai_response_content_prefers_output_text() {
    assert_eq!(
        parse_openai_response_content(br#"{"output_text":"hello codex"}"#),
        Ok("hello codex".to_owned())
    );
}

#[test]
fn provider_usage_accepts_openai_and_anthropic_shapes() {
    assert_eq!(
        token_usage_from_value(&serde_json::json!({
            "usage": {"prompt_tokens": 10, "completion_tokens": 3}
        })),
        Some(TokenUsage {
            input_tokens: 10,
            output_tokens: 3,
        })
    );
    assert_eq!(
        token_usage_from_value(&serde_json::json!({
            "usage": {"input_tokens": 7, "output_tokens": 2}
        })),
        Some(TokenUsage {
            input_tokens: 7,
            output_tokens: 2,
        })
    );
    assert_eq!(
        token_usage_from_value(&serde_json::json!({
            "response": {
                "usage": {"input_tokens": 9, "output_tokens": 4}
            }
        })),
        Some(TokenUsage {
            input_tokens: 9,
            output_tokens: 4,
        })
    );
}

#[test]
fn openai_request_bodies_include_non_auto_effort() {
    let chat = openai_chat_body("gpt-5.5", "hello", false, cortexfs::ModelEffort::High);
    let responses =
        openai_responses_body("gpt-5.5", "hello", true, cortexfs::ModelEffort::XHigh);

    assert!(chat.contains(r#""reasoning":{"effort":"high"}"#));
    assert!(responses.contains(r#""reasoning":{"effort":"xhigh"}"#));
    assert!(!openai_chat_body("gpt-5.5", "hello", false, cortexfs::ModelEffort::Auto)
        .contains("reasoning"));
}

#[test]
fn openai_response_content_parses_output_parts() {
    assert_eq!(
        parse_openai_response_content(
            br#"{"output":[{"content":[{"type":"output_text","text":"hello "},{"type":"output_text","text":"codex"}]}]}"#
        ),
        Ok("hello codex".to_owned())
    );
}

#[test]
fn agent_provider_messages_expose_only_tsh_as_native_tool() {
    let messages = provider_messages_for_agent(
        "what tools?",
        Some("coder"),
        "Always answer tersely.",
        &test_prompt_context(),
    );
    let system = messages
        .pointer("/0/content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(system.contains("only native callable tool is `tsh`"));
    assert!(system.contains("## AGENT Instructions"));
    assert!(system.contains("Always answer tersely."));
    assert!(system.contains("Project rule"));
    assert!(system.contains("name: rust"));
    assert!(system.contains("tool output"));
    assert!(system.contains("previous message"));
    assert!(system.contains("Do not claim direct access"));
    assert!(system.contains("Do not say that you cannot execute `tsh`"));
    assert!(system.contains("tsh load TOOL"));
    assert!(
        system.find("## Runtime Contract").unwrap_or(usize::MAX)
            < system.find("## AGENT Instructions").unwrap_or(usize::MAX)
    );
    assert_eq!(
        messages.pointer("/1/content").and_then(serde_json::Value::as_str),
        Some("what tools?")
    );

    let prompt = render_agent_system_prompt("coder", "", &test_prompt_context());
    assert!(prompt.contains("CortexFS agent `coder`"));
    assert!(prompt.contains("Do not mention hidden platform tools such as `image_gen`"));
}

#[test]
fn agent_prompt_template_controls_rendered_system_message() {
    let mut context = test_prompt_context();
    context.template = "agent={{agent}}\ninstructions={{agent_instructions}}\ncontract={{runtime_contract}}\n".to_owned();

    let prompt = render_agent_system_prompt("coder", "custom identity", &context);

    assert!(prompt.starts_with("agent=coder\ninstructions=custom identity\n"));
    assert!(prompt.contains("contract=You are CortexFS agent `coder`."));
    assert!(!prompt.contains("## Rules"));
}

fn test_prompt_context() -> AgentPromptContext {
    AgentPromptContext {
        template: DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        rules: "Project rule".to_owned(),
        skills: "- name: rust\n  description: Rust help\n  path: /skills/rust/SKILL.md\n"
            .to_owned(),
        tool_injection: "tool output".to_owned(),
        history_messages: "previous message".to_owned(),
        current_time_unix: "123".to_owned(),
    }
}

fn test_agent_run_config() -> AgentModelRunConfig {
    AgentModelRunConfig {
        agent: "coder".to_owned(),
        source: PathBuf::from("/tmp/cortexfs-test-source"),
        ctx_root: PathBuf::from("/tmp/cortexfs-test-ctx"),
        run: "r1".to_owned(),
        model: "main".to_owned(),
        model_path: PathBuf::from("/tmp/cortexfs-test-ctx/model/main"),
        system_prompt: String::new(),
        prompt_template: DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        rules: String::new(),
        skills: String::new(),
        current_time_unix: "123".to_owned(),
        tool_context: String::new(),
        suppress_model_error_events: false,
        debug_timing_start_unix_ms: None,
    }
}

#[test]
fn provider_runtime_driver_uses_responses_for_openai_agent_calls() {
    let config = test_provider_config_with_formats(
        "https://api.openai.com/v1",
        &["openai.chat", "openai.responses"],
    );

    assert_eq!(
        provider_runtime_driver(&config, false),
        ProviderRuntimeDriver::OpenAiChat
    );
    assert_eq!(
        provider_runtime_driver(&config, true),
        ProviderRuntimeDriver::OpenAiResponses
    );
}

#[test]
fn provider_runtime_driver_uses_responses_when_chat_is_absent() {
    let config = test_provider_config_with_formats(
        "https://api.openai.com/v1",
        &["openai.responses"],
    );

    assert_eq!(
        provider_runtime_driver(&config, false),
        ProviderRuntimeDriver::OpenAiResponses
    );
}

#[test]
fn provider_request_failure_includes_response_body() {
    let output = std::process::Output {
        status: std::os::unix::process::ExitStatusExt::from_raw(22 << 8),
        stdout: br#"{"error":"Missing API key"}"#.to_vec(),
        stderr: Vec::new(),
    };

    assert_eq!(
        provider_request_failure_message(&output),
        r#"provider request failed with exit status: 22: {"error":"Missing API key"}"#
    );
}

#[test]
fn provider_curl_output_rejects_oversized_stdout() -> Result<(), Box<dyn std::error::Error>> {
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "yes x | head -c {}",
            MAX_PROVIDER_RESPONSE_BYTES.saturating_add(1)
        ))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let result = wait_for_curl_json_output(child);

    assert!(matches!(result, Err(ref error) if error.contains("provider response exceeds")));
    Ok(())
}

#[test]
fn provider_curl_output_kills_child_after_oversized_stdout() -> Result<(), Box<dyn std::error::Error>>
{
    let started = Instant::now();
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "yes x | head -c {}; sleep 5",
            MAX_PROVIDER_RESPONSE_BYTES.saturating_add(1)
        ))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let result = wait_for_curl_json_output(child);

    assert!(matches!(result, Err(ref error) if error.contains("provider response exceeds")));
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn provider_transport_defaults_to_direct_base_url() {
    assert_eq!(
        provider_transport(
            &test_provider_config("https://api.openai.com/v1"),
            None
        ),
        Ok(ResolvedTransport::Direct {
            base_url: "https://api.openai.com/v1".to_owned()
        })
    );
}

#[test]
fn provider_config_from_dir_ignores_symlink_configs() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-symlink")?;
    let providers = root.join("providers.d");
    let outside = root.join("outside.json");
    fs::create_dir_all(&providers)?;
    fs::write(
        &outside,
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "formats": ["openai.chat"]
}
"#,
    )?;
    symlink(&outside, providers.join("local.json"))?;

    assert!(provider_config_from_dir(&providers, "local").is_none());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_config_file_reader_refuses_symlink_leaf() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-file-symlink")?;
    let providers = root.join("providers.d");
    let outside = root.join("outside.json");
    fs::create_dir_all(&providers)?;
    fs::write(&outside, "{}\n")?;
    symlink(&outside, providers.join("local.json"))?;
    let directory = open_runner_provider_config_dir(&providers)?;

    assert!(read_runner_provider_config_file(&directory, "local.json").is_err());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_config_from_dir_rejects_symlink_config_directory(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-dir-symlink")?;
    let providers = root.join("providers.d");
    let outside = root.join("outside-providers.d");
    fs::create_dir_all(&outside)?;
    fs::write(
        outside.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "formats": ["openai.chat"]
}
"#,
    )?;
    symlink(&outside, &providers)?;

    assert!(provider_config_from_dir(&providers, "local").is_none());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_config_from_dir_reads_plain_json_configs() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-plain")?;
    let providers = root.join("providers.d");
    fs::create_dir_all(&providers)?;
    fs::write(
        providers.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "formats": ["openai.responses"]
}
"#,
    )?;

    let config = provider_config_from_dir(&providers, "local");

    assert!(matches!(
        config,
        Some(ref config)
            if config.base_url == "http://127.0.0.1:8317/v1"
                && config.formats == ["openai.responses"]
    ));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_config_from_dir_rejects_duplicate_known_fields(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-duplicate")?;
    let providers = root.join("providers.d");
    fs::create_dir_all(&providers)?;
    fs::write(
        providers.join("local.json"),
        "{\"name\":\"local\",\"base_url\":\"http://127.0.0.1:8317/v1\",\"base_url\":\"http://evil/v1\"}\n",
    )?;

    assert!(provider_config_from_dir(&providers, "local").is_none());
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_runner_uses_absolute_curl_path() {
    let command = provider_curl_command();
    assert_eq!(command.get_program(), PROVIDER_CURL_BIN);
    assert_eq!(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["-q", "--config", "-"]
    );
    assert!(command.get_envs().next().is_none());
}

#[test]
fn runtime_provider_secret_reader_refuses_symlink_targets() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-runtime-secret-symlink")?;
    fs::create_dir_all(&root)?;
    let target = root.join("target");
    let link = root.join("secret");
    fs::write(&target, "runtime-secret\n")?;
    symlink(&target, &link)?;

    assert!(read_runtime_provider_secret_file(&link).is_err());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn runtime_provider_secret_reader_refuses_symlink_intermediate_dir(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-runtime-secret-symlink-intermediate")?;
    let outside = root.join("outside");
    fs::create_dir_all(&outside)?;
    fs::write(outside.join("secret"), "runtime-secret\n")?;
    symlink(&outside, root.join("runtime"))?;

    assert!(read_runtime_provider_secret_file(&root.join("runtime/secret")).is_err());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_secret_from_runtime_file_reads_only_matching_plain_file(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-runtime-secret-plain")?;
    fs::create_dir_all(&root)?;
    let path = root.join("secret");
    fs::write(&path, "runtime-secret\n")?;
    let path_text = path.display().to_string();
    let env = |name: &str| match name {
        "CTX_PROVIDER_SECRET_PROVIDER" => Ok("local".to_owned()),
        "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
        "CTX_PROVIDER_SECRET_PATH" => Ok(path_text.clone()),
        _ => Err(std::env::VarError::NotPresent),
    };

    assert_eq!(
        provider_secret_from_runtime_file_with_env("local", "default", env)?,
        Some("runtime-secret".to_owned())
    );
    assert_eq!(
        provider_secret_from_runtime_file_with_env("other", "default", env)?,
        None
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_secret_from_runtime_file_ignores_relative_path(
) -> Result<(), Box<dyn std::error::Error>> {
    let env = |name: &str| match name {
        "CTX_PROVIDER_SECRET_PROVIDER" => Ok("local".to_owned()),
        "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
        "CTX_PROVIDER_SECRET_PATH" => Ok("relative-secret".to_owned()),
        _ => Err(std::env::VarError::NotPresent),
    };

    assert_eq!(
        provider_secret_from_runtime_file_with_env("local", "default", env)?,
        None
    );
    Ok(())
}

#[test]
fn provider_secret_from_inherited_fd_reads_regular_secret_file(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-inherited-secret-fd")?;
    let path = root.join("secret");
    fs::write(&path, "fd-secret\n")?;
    let file = fs::File::open(&path)?;
    let fd = file.as_raw_fd().to_string();
    let env = |name: &str| match name {
        "CTX_PROVIDER_SECRET_PROVIDER" => Ok("local".to_owned()),
        "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
        "CTX_PROVIDER_SECRET_FD" => Ok(fd.clone()),
        _ => Err(std::env::VarError::NotPresent),
    };

    let secret = provider_secret_from_inherited_fd_with_env("local", "default", env)?;

    assert_eq!(secret, Some("fd-secret".to_owned()));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_secret_from_inherited_fd_ignores_standard_fds(
) -> Result<(), Box<dyn std::error::Error>> {
    for fd in ["0", "1", "2"] {
        let env = |name: &str| match name {
            "CTX_PROVIDER_SECRET_PROVIDER" => Ok("local".to_owned()),
            "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
            "CTX_PROVIDER_SECRET_FD" => Ok(fd.to_owned()),
            _ => Err(std::env::VarError::NotPresent),
        };

        assert_eq!(
            provider_secret_from_inherited_fd_with_env("local", "default", env)?,
            None
        );
    }
    Ok(())
}

#[test]
fn provider_secret_from_inherited_fd_rejects_non_regular_fd(
) -> Result<(), Box<dyn std::error::Error>> {
    let (read_fd, write_fd) = nix::unistd::pipe()?;
    let fd = read_fd.as_raw_fd().to_string();
    let env = |name: &str| match name {
        "CTX_PROVIDER_SECRET_PROVIDER" => Ok("local".to_owned()),
        "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
        "CTX_PROVIDER_SECRET_FD" => Ok(fd.clone()),
        _ => Err(std::env::VarError::NotPresent),
    };

    let result = provider_secret_from_inherited_fd_with_env("local", "default", env);

    assert!(matches!(result, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
    drop(read_fd);
    drop(write_fd);
    Ok(())
}

#[test]
fn provider_transport_uses_exact_http_route() {
    let config = test_provider_config("https://api.openai.com/v1");

    assert_eq!(
        provider_transport(
            &config,
            Some("group(office) -> http(http://127.0.0.1:8080/v1)\ndomain(openai.com) -> office\n")
        ),
        Ok(ResolvedTransport::Http {
            base_url: "http://127.0.0.1:8080/v1".to_owned()
        })
    );
}

#[test]
fn provider_transport_uses_wildcard_unix_route() {
    let config = test_provider_config("https://api.openai.com/v1");

    assert_eq!(
        provider_transport(
            &config,
            Some("group(local-socket) -> unix(/run/user/1000/cortexfs/proxy/openai.sock)\ndomain(openai.com) -> local-socket\n")
        ),
        Ok(ResolvedTransport::Unix {
            base_url: "http://localhost/v1".to_owned(),
            socket_path: "/run/user/1000/cortexfs/proxy/openai.sock".to_owned()
        })
    );
}

#[test]
fn provider_transport_rejects_relative_unix_socket_route() {
    let config = test_provider_config("https://api.openai.com/v1");

    assert!(provider_transport(
        &config,
        Some("group(local-socket) -> unix(relative.sock)\ndomain(openai.com) -> local-socket\n")
    )
    .is_err());
}

#[test]
fn provider_transport_rejects_control_character_unix_socket_route() {
    let config = test_provider_config("https://api.openai.com/v1");

    assert!(provider_transport(
        &config,
        Some("group(local-socket) -> unix(/run/cortexfs/\u{1b}proxy.sock)\ndomain(openai.com) -> local-socket\n")
    )
    .is_err());
}

#[test]
fn provider_transport_rejects_non_url_unix_base_url() {
    let config = test_provider_config("https://api.openai.com/v1");

    assert!(provider_transport(
        &config,
        Some("group(local-socket) -> unix(/run/cortexfs/proxy.sock, not-a-url)\ndomain(openai.com) -> local-socket\n")
    )
    .is_err());
}

#[test]
fn curl_config_quote_rejects_line_break_injection() {
    assert!(curl_config_quote("https://api.openai.com/v1").is_ok());
    assert!(curl_config_quote("https://api.openai.com/v1\noutput = /tmp/leak").is_err());
    assert!(curl_config_quote("Authorization: Bearer bad\rheader = injected").is_err());
    assert!(curl_config_quote("Authorization: Bearer \u{1b}]52;c;payload").is_err());
    assert!(curl_config_quote("abc\0def").is_err());
}

#[test]
fn provider_json_body_escapes_prompt_newlines_before_curl_config() {
    let body = openai_chat_body(
        "gpt-5.4",
        "first line\nsecond line",
        false,
        cortexfs::ModelEffort::Auto,
    );

    assert!(!body.contains('\n'));
    assert!(body.contains("\\n"));
    assert!(curl_config_quote(&body).is_ok());
}

#[test]
fn provider_route_selects_key_slot_by_model() {
    let config = test_provider_config("https://api.openai.com/v1");

    assert_eq!(
        provider_route(
            &config,
            "api.openai.com",
            "gpt-5.4",
            Some("group(paid) -> direct, key(office)\nmodel(gpt-*) -> paid\nfallback: direct\n")
        ),
        Ok(ProviderRoute {
            transport: ResolvedTransport::Direct {
                base_url: "https://api.openai.com/v1".to_owned()
            },
            key_slot: Some("office".to_owned())
        })
    );
}

#[test]
fn anthropic_message_content_parses_text_parts() {
    assert_eq!(
        parse_anthropic_message_content(
            br#"{"content":[{"type":"text","text":"hello "},{"type":"text","text":"claude"}]}"#
        ),
        Ok("hello claude".to_owned())
    );
}

fn test_provider_config(base_url: &str) -> RunnerProviderConfig {
    test_provider_config_with_formats(base_url, &[])
}

fn test_provider_config_with_formats(base_url: &str, formats: &[&str]) -> RunnerProviderConfig {
    RunnerProviderConfig {
        name: None,
        base_url: base_url.to_owned(),
        oauth: None,
        formats: formats.iter().map(|format| (*format).to_owned()).collect(),
    }
}

#[cfg(unix)]
#[test]
fn prompt_discovery_skips_symlinked_agents_files_and_skill_trees()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let _guard = env_lock()
        .lock()
        .map_err(|_error| std::io::Error::other("env lock poisoned"))?;
    let original_cwd = std::env::current_dir()?;
    let root = unique_temp_dir("prompt-symlinks")?;
    let workspace = root.join("workspace");
    let outside = root.join("outside");
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(outside.join("skills").join("leak"))?;
    fs::write(outside.join("secret.txt"), "SHOULD_NOT_LEAK_RULE")?;
    fs::write(
        outside.join("skills").join("leak").join("SKILL.md"),
        "---\nname: leak\ndescription: SHOULD_NOT_LEAK_SKILL\n---\nbody",
    )?;
    symlink(outside.join("secret.txt"), workspace.join("AGENTS.md"))?;
    fs::create_dir_all(workspace.join(".agents"))?;
    symlink(outside.join("skills"), workspace.join(".agents").join("skills"))?;

    std::env::set_current_dir(&workspace)?;
    let rules = collect_agent_rules();
    let skills = collect_skill_metadata(8_000);

    std::env::set_current_dir(original_cwd)?;
    let _ignored = fs::remove_dir_all(root);

    assert!(!rules.contains("SHOULD_NOT_LEAK_RULE"));
    assert!(!skills.contains("SHOULD_NOT_LEAK_SKILL"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn prompt_discovery_enforces_file_size_limits() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = env_lock()
        .lock()
        .map_err(|_error| std::io::Error::other("env lock poisoned"))?;
    let original_cwd = std::env::current_dir()?;
    let root = unique_temp_dir("prompt-limits")?;
    let workspace = root.join("workspace");
    fs::create_dir_all(workspace.join(".agents").join("skills").join("large"))?;
    fs::write(workspace.join("AGENTS.md"), "A".repeat(70 * 1024))?;
    fs::write(
        workspace
            .join(".agents")
            .join("skills")
            .join("large")
            .join("SKILL.md"),
        format!("---\nname: large\ndescription: {}\n---\n", "B".repeat(20 * 1024)),
    )?;

    std::env::set_current_dir(&workspace)?;
    let rules = collect_agent_rules();
    let skills = collect_skill_metadata(8_000);

    std::env::set_current_dir(original_cwd)?;
    let _ignored = fs::remove_dir_all(root);

    assert!(!rules.contains(&"A".repeat(1024)));
    assert!(!skills.contains("large"));
    Ok(())
}
