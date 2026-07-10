#![forbid(unsafe_code)]
#![expect(
    clippy::allow_attributes,
    reason = "allow target-specific lint exceptions"
)]
#![allow(
    unfulfilled_lint_expectations,
    reason = "expected target-specific lint results"
)]
#![expect(
    clippy::wildcard_imports,
    reason = "uniform submodules with wildcard imports"
)]
#![expect(clippy::redundant_pub_crate, reason = "submodule visibility alignment")]
#![expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "internal structs with scoped fields"
)]
#![expect(clippy::module_inception, reason = "allow submodule self name")]

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

#[cfg(test)]
pub(crate) use cortexfs::LayoutPathRole;
pub(crate) use cortexfs::authority_helpers::atomic_replace_text_with_mode;
pub(crate) use cortexfs::{
    AbiPathKind, AgentControlIssue, AgentPromptContext, AgentRuntimeView, AgentScheduleIssue,
    AgentScheduleNode, AgentScheduleRecordError, CTX_ROOT, ChildContextRecordError,
    ChildContextStatus, ContextJsonlIssue, ContextPackIssue, ControlLineIssue,
    DEFAULT_AGENT_PROMPT_TEMPLATE, EventStreamIssue, MANUAL_INDEX, MANUAL_INDEX_FILE,
    MANUAL_MAN_DIR, MANUAL_SHARED_DIR, MAX_SOCKET_FRAME_BYTES, MessageStreamIssue,
    ModelCapabilityIssue, ModelDriverRouteError, ModelEffort, ModelFallbackIssue, MountMode,
    MountTable, ObjectClass, ObjectLayoutIssue, PathLayoutIssue, PolicyV0, REFERENCE_TREE_VERSION,
    ROOT_ENTRIES, SessionControlIssue, SessionIndexIssue, SessionIndexKind, SessionLayoutIssue,
    SharedQueueLayoutIssue, SocketSessionScope, ToolExecutionAuthority, ToolPath, ToolSchemaIssue,
    TrajectoryMapError, advance_agent_schedule_from_parent_context, agent_schedule_nodes,
    authorize_tool_execution, bootstrap_state_matches_target, classify_abi_path,
    collect_agent_rules, collect_skill_metadata,
    completed_agent_schedule_nodes_from_parent_context, cortexfs_manual, current_time_unix,
    default_agent_model_for_name, default_agent_tool_context, derive_agent_runtime_view,
    ensure_durable_session_layout, ensure_v1_reference_tree, ensure_v1_runtime_models,
    format_bootstrap_plan_lines, inspect_agent_control, inspect_agent_schedule_json,
    inspect_context_jsonl, inspect_context_pack_json, inspect_event_stream_jsonl,
    inspect_message_stream_jsonl, inspect_model_capabilities, inspect_object_layout,
    inspect_session_control, inspect_session_index, inspect_session_layout,
    inspect_shared_queue_layout, inspect_tool_schema_json, install_executable_object_wrapper,
    is_dedicated_worker_agent_name, is_executable_file, is_managed_reference_agent_wrapper,
    is_model_name, is_object_name, is_worker_agent_name, list_present_retired_reference_agents,
    parse_abi_path, parse_model_driver_routes, parse_model_fallback, plan_reference_tree_upgrade,
    policy_subject_from_label, read_bootstrap_state, ready_agent_schedule_nodes,
    record_child_result_to_parent_context, render_agent_system_prompt, run_core_tool_cli_with_root,
    skill_metadata_budget_from_env, trajectory_from_session_dir, validate_trajectory,
};
use nix::sys::termios::{SetArg, Termios, cfmakeraw, tcgetattr, tcsetattr};
use serde::Deserialize;

#[path = "shared/current-uid.rs"]
pub mod current_uid;
#[path = "shared/json.rs"]
pub mod json;
#[path = "shared/limited-input.rs"]
pub mod limited_input;
#[path = "shared/stderr.rs"]
pub mod stderr;
#[path = "shared/terminal-io.rs"]
pub mod terminal_io;
pub(crate) use agent::*;
pub(crate) use basic::*;
pub(crate) use check::*;
pub(crate) use cortexfs::plain_fs::open_plain_directory;
pub(crate) use create_plain_dir::*;
pub(crate) use current_uid::*;
pub(crate) use doctor::*;
pub(crate) use format::*;
pub(crate) use json::*;
pub(crate) use limited_input::*;
pub(crate) use model_alias::*;
pub(crate) use objects_socket::*;
pub(crate) use output_mount::*;
pub(crate) use parse::*;
pub(crate) use proc_fd::*;
pub(crate) use provider::*;
pub(crate) use schedule::*;
pub(crate) use stale_socket::*;
pub(crate) use stderr::*;
pub(crate) use terminal_io::*;
pub(crate) use util::*;

pub(crate) fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            let _ignored = write_error(&cli_error_line(&error));
            ExitCode::from(error.code)
        }
    }
}

pub(crate) fn cli_error_line(error: &CliError) -> String {
    format!("ctx: {}", terminal_safe_text(&error.message))
}

#[path = "ctx/parse.rs"]
pub mod parse;

#[path = "ctx/agent.rs"]
pub mod agent;

#[path = "ctx/output-mount.rs"]
pub mod output_mount;

#[path = "shared/create-plain-dir.rs"]
pub mod create_plain_dir;
#[path = "shared/model-alias.rs"]
pub mod model_alias;
#[path = "shared/plain-dir.rs"]
pub mod plain_dir;
#[path = "shared/small-text.rs"]
pub mod small_text;
#[path = "shared/stale-socket.rs"]
pub mod stale_socket;
pub(crate) use small_text::read_small_plain_text_file;

#[path = "shared/no-follow-fs.rs"]
pub mod no_follow_fs;
pub(crate) use no_follow_fs::open_regular_file_no_follow;

#[path = "shared/proc-fd.rs"]
pub mod proc_fd;

#[path = "ctx/objects-socket.rs"]
pub mod objects_socket;

#[path = "ctx/doctor.rs"]
pub mod doctor;

#[path = "ctx/provider.rs"]
pub mod provider;

#[path = "ctx/file/basic.rs"]
pub mod basic;

#[path = "ctx/file/check.rs"]
pub mod check;

#[path = "ctx/schedule.rs"]
pub mod schedule;

#[path = "ctx/format.rs"]
pub mod format;

#[path = "ctx/util.rs"]
pub mod util;

#[cfg(test)]
#[expect(unused_qualifications, reason = "tests use qualified paths")]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/ctx_tests.rs"
    ));
}
