use super::{
    ObjectPath, OpenAiStreamEvent, ProviderRoute, ProviderRuntimeDriver, ResolvedTransport,
    RunnerProviderConfig, TokenUsage, agent_tool_call_from_value, is_passthrough_tool,
    missing_model_message, model_candidates, openai_chat_body, openai_responses_body,
    openai_stream_event, parse_anthropic_message_content, parse_openai_response_content,
    provider_messages_for_agent, provider_request_failure_message, provider_route,
    provider_runtime_driver, provider_transport, resolve_model_alias, resolved_model_path, run,
    run_cli_tool_to_writer, token_usage_from_value, tool_call_from_text, validate_agent_tsh_args,
    write_model_text_or_tool_call,
};
use cortexfs::{
    AgentPromptContext, DEFAULT_AGENT_PROMPT_TEMPLATE, collect_agent_rules, collect_skill_metadata,
    render_agent_system_prompt,
};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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
fn openai_stream_event_extracts_responses_delta_text() {
    let event = openai_stream_event(
        r#"data: {"type":"response.output_text.delta","delta":"hel"}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text == "hel"));
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
    assert!(system.contains("tsh load TOOL"));
    assert_eq!(
        messages.pointer("/1/content").and_then(serde_json::Value::as_str),
        Some("what tools?")
    );

    let prompt = render_agent_system_prompt("coder", "", &test_prompt_context());
    assert!(prompt.contains("CortexFS agent `coder`"));
    assert!(!prompt.contains("image_gen"));
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
