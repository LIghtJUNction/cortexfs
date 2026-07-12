use super::{
    AgentModelRunConfig, AgentModelRunOutcome, AgentToolBwrapArgs, AgentToolCall, AgentToolSandbox,
    BWRAP_PROGRAM, MAX_AGENT_MODEL_FRAME_BYTES, MAX_AGENT_MODEL_FRAMES,
    MAX_AGENT_TOOL_CONTEXT_BYTES, MAX_AGENT_TOOL_OUTPUT_BYTES, MAX_CHILD_STDERR_BYTES,
    MAX_PROVIDER_RESPONSE_BYTES, MAX_PROVIDER_STREAM_LINE_BYTES, MAX_RUNNER_PROVIDER_CONFIG_BYTES,
    MAX_RUNNER_STDIN_INPUT_BYTES, MAX_STREAM_TOOL_CALL_BUFFER_BYTES, MAX_TOOL_RESULT_CHARS,
    ObjectPath, OpenAiStreamEvent, OpenAiStreamTextEmitter, OpenAiToolCallStream,
    PROVIDER_CURL_BIN, ProviderCredential, ProviderRoute, ProviderRuntimeDriver, ResolvedTransport,
    RunnerProviderConfig, TokenUsage, agent_model_timeout_seconds_from_env, agent_tool_bwrap_args,
    agent_tool_call_from_value, agent_tool_timeout_seconds_from_env, collect_child_stderr,
    curl_config_quote, execute_agent_tool_call, first_tool_call, is_passthrough_tool,
    is_regular_file_no_follow, missing_model_message, model_candidates, nested_control_environment,
    nested_control_socket_is_plain, normalize_agent_model_frame, open_executable_no_follow,
    openai_api_key, openai_chat_body, openai_chat_body_with_agent_tools, openai_responses_body,
    openai_stream_event, parse_anthropic_message_content, parse_openai_chat_content,
    parse_openai_response_content, passthrough_tool_program, proc_fd_path,
    provider_config_from_dir, provider_curl_command, provider_messages_for_agent,
    provider_request_failure_message, provider_route, provider_runtime_driver,
    provider_secret_from_inherited_fd_with_env, provider_secret_from_runtime_file_with_env,
    provider_secret_from_runtime_value_with_env, provider_transport, read_limited_input_text,
    read_provider_stream_line, read_runtime_provider_secret_file, read_small_plain_text_file,
    resolve_model_alias, resolved_model_path, run, run_agent_model_once,
    run_agent_model_once_with_timeout, run_agent_tool_loop, run_agent_tool_process_with_timeout,
    run_core_tool_cli, run_passthrough_tool, spawn_child_stderr_reader, split_object_args,
    token_usage_from_value, tool_call_from_text, tool_terminal_done_line,
    tool_terminal_running_line, trim_tool_result, validate_agent_tsh_args,
    validate_nested_control_values, visible_workspace_source, wait_for_curl_json_output,
    write_model_text_or_tool_call,
};
use cortexfs::{
    AgentPromptContext, DEFAULT_AGENT_PROMPT_TEMPLATE, agent_runtime_contract, collect_agent_rules,
    collect_skill_metadata, default_agent_tool_context, render_agent_system_prompt,
    support::plain::open_plain_directory,
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

mod config;
mod execution;
mod model;
mod native;
mod parsing;
mod process;
mod prompt;
mod runtime;
mod stream;
mod toolloop;
