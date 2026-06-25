use super::{
    is_passthrough_tool, openai_stream_event, provider_key_names, provider_messages_for_agent,
    resolve_model_alias, resolved_model_path, run, run_cli_tool_to_writer, ObjectPath,
    OpenAiStreamEvent, RunnerProviderConfig,
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
        run(vec![OsString::from("/ctx/model/openai/gpt-4o")]),
        Err("missing provider: openai".to_owned())
    );
}

#[test]
fn proxy_model_emits_portable_manual_request() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    cortexfs::run_proxy_model(["debug this agent"], &mut output)?;
    let output = String::from_utf8(output)?;

    assert!(output.contains(r#""model":"debug/proxy""#));
    assert!(output.contains("CortexFS debug proxy request"));
    assert!(output.contains("cortexfs_proxy_version"));
    assert!(output.contains("debug this agent"));
    Ok(())
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
