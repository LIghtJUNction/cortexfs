use super::exec::{
    AgentToolBwrapArgs, AgentToolSandbox, agent_tool_bwrap_args,
    run_agent_tool_process_with_timeout, visible_workspace_source,
};
use super::{
    AgentModelRunConfig, AgentToolCall, BWRAP_PROGRAM, ExecError, MAX_AGENT_MODEL_FRAME_BYTES,
    MAX_AGENT_MODEL_FRAMES, MAX_AGENT_TOOL_OUTPUT_BYTES, MAX_CHILD_STDERR_BYTES,
    MAX_PROVIDER_RESPONSE_BYTES, MAX_PROVIDER_STREAM_LINE_BYTES, MAX_RUNNER_PROVIDER_CONFIG_BYTES,
    MAX_RUNNER_STDIN_INPUT_BYTES, MAX_STREAM_TOOL_CALL_BUFFER_BYTES, MAX_TOOL_RESULT_CHARS,
    ObjectPath, OpenAiStreamEvent, OpenAiStreamTextEmitter, OpenAiToolCallStream,
    PROVIDER_CURL_BIN, ProviderCandidateAdmission, ProviderCredential, ProviderRoute,
    ProviderRuntimeDriver, ResolvedTransport, RunnerProviderConfig, TokenUsage, admit_agent_prompt,
    admit_provider_candidate, agent_model_command, agent_model_timeout_seconds_from_env,
    agent_tool_call_from_value, agent_tool_timeout_seconds_from_env, collect_child_stderr,
    curl_config_quote, first_tool_call, frames_have_error, is_passthrough_tool,
    is_regular_file_no_follow, missing_model_message, model_candidates,
    normalize_agent_model_frame, open_executable_no_follow, openai_api_key, openai_chat_body,
    openai_chat_body_with_agent_tools, openai_responses_body, openai_stream_event,
    parse_agent_context_budget, parse_agent_window_environment, parse_anthropic_message_content,
    parse_openai_chat_content, parse_openai_response_content, passthrough_tool_program,
    proc_fd_path, provider_config_from_dir, provider_curl_command, provider_egress_transport,
    provider_messages_for_agent, provider_request_attempts, provider_request_failure_message,
    provider_route, provider_runtime_driver, provider_secret_from_inherited_fd_with_env,
    provider_secret_from_runtime_file_with_env, provider_secret_from_runtime_value_with_env,
    provider_target, provider_transport, read_limited_input_text, read_provider_stream_line,
    read_runtime_provider_secret_file, read_small_plain_text_file, reset_provider_request_attempts,
    resolve_model_alias, resolved_model_path, run, run_agent_model_once,
    run_agent_model_once_with_timeout, run_core_tool_cli, run_passthrough_tool,
    serialized_agent_messages, spawn_child_stderr_reader, split_object_args,
    token_usage_from_value, tool_call_from_text, transport_allows_unauthenticated,
    trim_tool_result, validate_agent_tsh_args, wait_for_curl_json_output,
    write_model_text_or_tool_call, write_model_usage,
};
use cortexfs::{
    AgentPromptContext, AgentWindowBudget, AgentWindowSetting, DEFAULT_AGENT_PROMPT_TEMPLATE,
    ModelContextLimit, agent_runtime_contract, collect_agent_rules, collect_skill_metadata,
    default_agent_tool_context, render_agent_system_prompt, support::plain::open_plain_directory,
};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{Cursor, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
pub(crate) fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(unix)]
pub(crate) fn unique_temp_dir(name: &str) -> std::io::Result<PathBuf> {
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
pub(crate) fn write_executable_script(
    path: &Path,
    content: impl AsRef<[u8]>,
) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(content.as_ref())?;
    file.sync_all()?;
    drop(file);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

pub(crate) fn write_sdk_tool(path: &Path, tool: &str, text: &str) -> std::io::Result<()> {
    let frames = [
        serde_json::json!({"type":"start", "run":"r1", "tool":tool}),
        serde_json::json!({
            "type":"message", "run":"r1", "role":"tool",
            "content":[{"type":"text", "text":text}]
        }),
        serde_json::json!({"type":"done", "run":"r1", "status":"ok"}),
    ]
    .map(|frame| crate::shell_single_quote(&frame.to_string()))
    .join(" ");
    write_executable_script(path, format!("#!/bin/sh\nprintf '%s\\n' {frames}\n"))
}

pub(super) fn test_agent_tool_config(
    config: &AgentModelRunConfig,
) -> super::exec::AgentToolExecutionConfig<'_> {
    super::exec::AgentToolExecutionConfig {
        agent: &config.agent,
        source: &config.source,
        ctx_root: &config.ctx_root,
        run: &config.run,
        session: "default",
        control: None,
        cancel: None,
    }
}

pub(super) fn execute_prepared_agent_tool_call(
    config: &AgentModelRunConfig,
    call: &AgentToolCall,
) -> Result<String, ExecError> {
    let config = test_agent_tool_config(config);
    super::exec::prepare_agent_tool_call(&config, call)?.execute(&config)
}

#[cfg(unix)]
pub(crate) fn short_unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cfs-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

#[cfg(unix)]
pub(crate) fn agent_tool_fixture(name: &str, tool: &str) -> std::io::Result<(PathBuf, PathBuf)> {
    let root = short_unique_temp_path(name);
    let _ignored = fs::remove_dir_all(&root);
    let control = root.join("agent/coder.d");
    let tool_control = root.join("tool").join(format!("{tool}.d"));
    fs::create_dir_all(&control)?;
    fs::create_dir_all(&tool_control)?;
    for (file, value) in [
        ("owner", "1000\n"),
        ("uid", "1000\n"),
        ("gid", "1000\n"),
        ("groups", "1000\n"),
        ("perm", "rwx\n"),
        ("label", "user_u:agent_r:coder_t:s0\n"),
        ("iso", "shared\n"),
        ("parent", "\n"),
        ("life", "owned\n"),
        ("root", "/ctx/home/1000/agent/coder/root\n"),
        ("cwd", "/workspace\n"),
        ("env", "\n"),
        ("abi", "sdk-envelope-v1\n"),
        ("model", "main\n"),
    ] {
        fs::write(control.join(file), value)?;
    }
    fs::write(control.join("window"), "auto\n")?;
    let model_control = root.join("model/local/chat.d");
    fs::create_dir_all(&model_control)?;
    fs::write(model_control.join("limit"), "unknown\n")?;
    symlink("/ctx/model/local/chat", root.join("model/main"))?;
    for (file, value) in [
        ("status", "idle\n"),
        ("pid", "\n"),
        ("log", "\n"),
        ("meta.json", "{}\n"),
    ] {
        fs::write(control.join(file), value)?;
    }
    fs::write(
        control.join("path"),
        format!("{}\n", root.join("tool").display()),
    )?;
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
        format!("allow coder_t model:main use\nallow coder_t tool:{tool} execute\n"),
    )?;
    Ok((root, tool_control))
}

mod config;
mod execution;
mod model;
mod native;
mod parsing;
mod process;
mod prompt;
mod runtime;
mod stream;
