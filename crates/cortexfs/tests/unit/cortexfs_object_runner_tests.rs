use super::{
    AgentToolCall, ObjectPath, OpenAiStreamEvent, ProviderCredential, ProviderRoute,
    ProviderRuntimeDriver, ResolvedTransport, RunnerProviderConfig, TokenUsage,
    agent_tool_call_from_value,
    execute_agent_tool_call, is_passthrough_tool, open_executable_no_follow,
    passthrough_tool_program,
    run_agent_model_once, run_agent_model_once_with_timeout, missing_model_message,
    is_regular_file_no_follow, model_candidates, openai_api_key, openai_chat_body,
    openai_responses_body, openai_stream_event, parse_anthropic_message_content,
    parse_openai_response_content, curl_config_quote, open_runner_provider_config_dir,
    provider_config_from_dir,
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


include!("cortexfs_object_runner_tests/object_model.rs");
include!("cortexfs_object_runner_tests/tool_parsing.rs");
include!("cortexfs_object_runner_tests/tool_loop.rs");
include!("cortexfs_object_runner_tests/agent_process.rs");
include!("cortexfs_object_runner_tests/tool_execution.rs");
include!("cortexfs_object_runner_tests/provider_stream.rs");
include!("cortexfs_object_runner_tests/provider_runtime.rs");
include!("cortexfs_object_runner_tests/provider_config.rs");
include!("cortexfs_object_runner_tests/prompt_discovery.rs");
