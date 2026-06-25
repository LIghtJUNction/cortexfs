use super::{
    agent_system_prompt, is_passthrough_tool, openai_stream_event, provider_key_names,
    provider_messages_for_agent, resolve_model_alias, resolved_model_path, run,
    run_cli_tool_to_writer, AgentPromptContext, ObjectPath, OpenAiStreamEvent, RunnerProviderConfig,
};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

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
        run(vec![OsString::from("/ctx/model/openai/gpt-4o")]),
        Err("missing provider: openai".to_owned())
    );
}

#[test]
fn model_alias_resolves_only_ctx_model_objects() {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runner-model-alias-ok-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("model")).expect("create model dir");
    symlink("/ctx/model/debug/echo", root.join("model/main")).expect("create model alias");

    assert_eq!(
        resolve_model_alias(&root, "main"),
        Ok("debug/echo".to_owned())
    );
    assert_eq!(
        resolved_model_path(&root, "main"),
        Ok(root.join("model/debug/echo"))
    );

    let _ignored = fs::remove_dir_all(root);
}

#[test]
fn model_alias_rejects_cross_class_symlink_target() {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runner-model-alias-bad-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("model")).expect("create model dir");
    symlink("../tool/shell.exec", root.join("model/main")).expect("create model alias");

    assert_eq!(
        resolve_model_alias(&root, "main"),
        Err("invalid model alias target: main".to_owned())
    );

    let _ignored = fs::remove_dir_all(root);
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
fn runner_recognizes_interactive_tool_passthroughs() {
    assert!(is_passthrough_tool("bash"));
    assert!(is_passthrough_tool("tmux"));
    assert!(is_passthrough_tool("zellij"));
    assert!(is_passthrough_tool("tsh"));
    assert!(!is_passthrough_tool("shell.exec"));
    assert!(!is_passthrough_tool("fs.read"));
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
fn openai_stream_event_accepts_done_marker() {
    assert!(matches!(
        openai_stream_event("data: [DONE]"),
        Ok(OpenAiStreamEvent::Done)
    ));
}

#[test]
fn openai_stream_event_does_not_mix_reasoning_into_answer_text() {
    let event = openai_stream_event(
        r#"data: {"choices":[{"delta":{"reasoning_content":"hidden"}}]}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text.is_empty()));
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

    let prompt = agent_system_prompt("coder", "", &test_prompt_context());
    assert!(prompt.contains("CortexFS agent `coder`"));
    assert!(!prompt.contains("image_gen"));
}

fn test_prompt_context() -> AgentPromptContext {
    AgentPromptContext {
        template: super::default_agent_prompt_template(),
        rules: "Project rule".to_owned(),
        skills: "- name: rust\n  description: Rust help\n  path: /skills/rust/SKILL.md\n"
            .to_owned(),
        tool_injection: "tool output".to_owned(),
        history_messages: "previous message".to_owned(),
        current_time_unix: "123".to_owned(),
    }
}

#[test]
fn provider_key_names_accept_configured_and_host_fallbacks() {
    assert_eq!(
        provider_key_names(&RunnerProviderConfig {
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key_env: Some("CORTEXFS_OPENAI_KEY".to_owned()),
        }),
        vec![
            "CORTEXFS_OPENAI_KEY".to_owned(),
            "OPENAI_COM_API_KEY".to_owned(),
            "API_OPENAI_COM_API_KEY".to_owned(),
        ]
    );
}

#[test]
fn provider_key_names_reject_invalid_configured_names() {
    assert_eq!(
        provider_key_names(&RunnerProviderConfig {
            base_url: "https://localhost:11434/v1".to_owned(),
            api_key_env: Some("bad-name".to_owned()),
        }),
        vec!["LOCALHOST_API_KEY".to_owned()]
    );
}
