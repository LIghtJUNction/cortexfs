#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::net::Shutdown;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nix::libc;

use cortexfs::{
    AbiPathKind, AgentControlIssue, AgentPromptContext, AgentRuntimeView, AgentScheduleIssue,
    AgentScheduleNode, AgentScheduleRecordError, CTX_ROOT, ChildContextRecordError,
    ChildContextStatus, ContextJsonlIssue, ContextPackIssue, DEFAULT_AGENT_PROMPT_TEMPLATE,
    EventStreamIssue, MANUAL_INDEX, MANUAL_INDEX_FILE, MANUAL_MAN_DIR, MANUAL_SHARED_DIR,
    MAX_SOCKET_FRAME_BYTES, MessageStreamIssue, ModelCapabilityIssue, ModelDriverRouteError,
    ModelEffort, ModelFallbackIssue, MountMode, MountTable, ObjectClass, ObjectLayoutIssue,
    PolicyV0, ROOT_ENTRIES, SessionControlIssue, SessionIndexIssue, SessionIndexKind,
    SessionLayoutIssue, SharedQueueLayoutIssue, SocketSessionScope, ToolExecutionAuthority,
    ToolPath, ToolSchemaIssue, advance_agent_schedule_from_parent_context, agent_schedule_nodes,
    authorize_tool_execution, classify_abi_path, collect_agent_rules, collect_skill_metadata,
    completed_agent_schedule_nodes_from_parent_context, cortexfs_manual, current_time_unix,
    default_agent_model_for_name, default_agent_tool_context, derive_agent_runtime_view,
    ensure_durable_session_layout, ensure_v1_reference_tree, ensure_v1_runtime_models,
    inspect_agent_control, inspect_agent_schedule_json, inspect_context_jsonl,
    inspect_context_pack_json, inspect_event_stream_jsonl, inspect_message_stream_jsonl,
    inspect_model_capabilities, inspect_object_layout, inspect_session_control,
    inspect_session_index, inspect_session_layout, inspect_shared_queue_layout,
    inspect_tool_schema_json, install_executable_object_wrapper, is_dedicated_worker_agent_name,
    is_executable_file, is_model_name, is_object_name, is_worker_agent_name, parse_abi_path,
    parse_model_driver_routes, parse_model_fallback, ready_agent_schedule_nodes,
    record_child_result_to_parent_context, render_agent_system_prompt, run_core_tool_cli_with_root,
    skill_metadata_budget_from_env,
};
use nix::sys::termios::{SetArg, Termios, cfmakeraw, tcgetattr, tcsetattr};
use serde::Deserialize;

include!("shared/stderr.rs");
include!("shared/json.rs");
include!("shared/current_uid.rs");
include!("shared/terminal_io.rs");
include!("shared/limited_input.rs");
include!("../policy_subject.rs");

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            let _ignored = write_error(&cli_error_line(&error));
            ExitCode::from(error.code)
        }
    }
}

fn cli_error_line(error: &CliError) -> String {
    format!("ctx: {}", terminal_safe_text(&error.message))
}

include!("ctx/parse.rs");

include!("ctx/agent.rs");

include!("ctx/output_mount.rs");

include!("shared/plain_dir.rs");
include!("shared/create_plain_dir.rs");
include!("shared/stale_socket.rs");
include!("shared/model_alias.rs");
include!("shared/proc_fd.rs");

include!("ctx/objects_socket.rs");

include!("ctx/doctor.rs");

include!("ctx/provider.rs");

include!("ctx/file/basic.rs");

include!("ctx/file/check.rs");

include!("ctx/schedule.rs");

include!("ctx/format.rs");

include!("ctx/util.rs");

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/ctx_tests.rs"
    ));
}
