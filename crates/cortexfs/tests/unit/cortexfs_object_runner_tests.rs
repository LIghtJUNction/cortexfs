use super::{
    agent_system_prompt, collect_agent_rules, collect_skill_metadata, is_passthrough_tool,
    openai_stream_event, provider_key_names, provider_messages_for_agent, run,
    run_cli_tool_to_writer, AgentPromptContext, ObjectPath, OpenAiStreamEvent, RunnerProviderConfig,
};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[cfg(unix)]
fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(unix)]
fn unique_temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "cortexfs-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
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
        run(vec![OsString::from("/ctx/model/openai/gpt-4o")]),
        Err("missing provider: openai".to_owned())
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

#[cfg(unix)]
#[test]
fn prompt_discovery_skips_symlinked_agents_files_and_skill_trees() {
    use std::os::unix::fs::symlink;

    let _guard = env_lock().lock().unwrap();
    let original_cwd = std::env::current_dir().unwrap();
    let root = unique_temp_dir("prompt-symlinks");
    let workspace = root.join("workspace");
    let outside = root.join("outside");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(outside.join("skills").join("leak")).unwrap();
    fs::write(outside.join("secret.txt"), "SHOULD_NOT_LEAK_RULE").unwrap();
    fs::write(
        outside.join("skills").join("leak").join("SKILL.md"),
        "---\nname: leak\ndescription: SHOULD_NOT_LEAK_SKILL\n---\nbody",
    )
    .unwrap();
    symlink(outside.join("secret.txt"), workspace.join("AGENTS.md")).unwrap();
    fs::create_dir_all(workspace.join(".agents")).unwrap();
    symlink(outside.join("skills"), workspace.join(".agents").join("skills")).unwrap();

    std::env::set_current_dir(&workspace).unwrap();
    let rules = collect_agent_rules();
    let skills = collect_skill_metadata(8_000);

    std::env::set_current_dir(original_cwd).unwrap();
    let _ignored = fs::remove_dir_all(root);

    assert!(!rules.contains("SHOULD_NOT_LEAK_RULE"));
    assert!(!skills.contains("SHOULD_NOT_LEAK_SKILL"));
}

#[cfg(unix)]
#[test]
fn prompt_discovery_enforces_file_size_limits() {
    let _guard = env_lock().lock().unwrap();
    let original_cwd = std::env::current_dir().unwrap();
    let root = unique_temp_dir("prompt-limits");
    let workspace = root.join("workspace");
    fs::create_dir_all(workspace.join(".agents").join("skills").join("large")).unwrap();
    fs::write(workspace.join("AGENTS.md"), "A".repeat(70 * 1024)).unwrap();
    fs::write(
        workspace
            .join(".agents")
            .join("skills")
            .join("large")
            .join("SKILL.md"),
        format!("---\nname: large\ndescription: {}\n---\n", "B".repeat(20 * 1024)),
    )
    .unwrap();

    std::env::set_current_dir(&workspace).unwrap();
    let rules = collect_agent_rules();
    let skills = collect_skill_metadata(8_000);

    std::env::set_current_dir(original_cwd).unwrap();
    let _ignored = fs::remove_dir_all(root);

    assert!(!rules.contains(&"A".repeat(1024)));
    assert!(!skills.contains("large"));
}
