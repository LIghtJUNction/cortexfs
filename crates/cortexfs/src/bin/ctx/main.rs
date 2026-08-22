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
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cortexfs::support::terminal::terminal_safe_text;
use nix::libc;

#[cfg(test)]
pub(crate) use cortexfs::LayoutPathRole;
pub(crate) use cortexfs::authority::helpers::{
    atomic_create_text_with_mode, atomic_replace_text_preserving_metadata,
    atomic_replace_text_preserving_metadata_if_matches,
};
pub(crate) use cortexfs::{
    AbiPathKind, AgentControlIssue, AgentLaunchCommand, AgentLaunchMount, AgentLaunchRequest,
    AgentPromptContext, AgentRuntimeView, AgentScheduleIssue, AgentScheduleNode,
    AgentScheduleRecordError, AgentUnixIdentity, BootstrapAction, CHANNEL_CONTROL_FILES, CTX_ROOT,
    ChildContextLease, ChildContextRecordError, ChildContextStatus, ChildHandoffReceipt,
    ContextJsonlIssue, ContextPackIssue, ControlLineIssue, DEFAULT_AGENT_PROMPT_TEMPLATE,
    EventStreamIssue, MANUAL_INDEX, MANUAL_INDEX_FILE, MANUAL_MAN_DIR, MANUAL_SHARED_DIR,
    MAX_SOCKET_FRAME_BYTES, MessageStreamIssue, ModelCapabilityIssue, ModelDriverRouteError,
    ModelEffort, MountTable, ObjectClass, ObjectLayoutIssue, PathLayoutIssue, PolicyEvaluator,
    PolicyV0, REFERENCE_TREE_VERSION, ROOT_ENTRIES, SessionControlIssue, SessionIndexGuard,
    SessionIndexIssue, SessionIndexKind, SessionLayoutIssue, SharedQueueLayoutIssue,
    SocketSessionScope, ToolPath, ToolSchemaIssue, TrajectoryIssue, TrajectoryMapError,
    acquire_child_context_lease, advance_agent_schedule_from_parent_context, agent_schedule_nodes,
    agent_terminal_unit, bootstrap_state_matches_target, child_context_lease_status,
    child_handoff_receipt, claim_child_handoff_active_with_lease, classify_abi_path,
    collect_agent_rules, collect_skill_metadata, columnar, compare_and_update_session_index,
    completed_agent_schedule_nodes_from_parent_context, cortexfs_manual, current_time_unix,
    default_agent_model_for_name, default_agent_tool_context, derive_agent_runtime_view,
    ensure_durable_session_layout, ensure_reference_tree, ensure_runtime_models,
    finish_child_result_with_lease, format_bootstrap_plan_lines, inspect_agent_control,
    inspect_agent_schedule_json, inspect_context_jsonl, inspect_context_pack_json,
    inspect_event_stream_jsonl, inspect_message_stream_jsonl, inspect_model_capabilities,
    inspect_object_layout, inspect_session_control, inspect_session_index, inspect_session_layout,
    inspect_shared_queue_layout, inspect_tool_schema_json, invocation_id,
    is_dedicated_worker_agent_name, is_executable_file, is_managed_reference_agent_wrapper,
    is_model_alias, is_model_name, is_object_name, is_worker_agent_name, launch_process_for,
    list_present_retired_reference_agents, parse_abi_path, parse_model_driver_routes,
    pin_storage_source, plan_reference_tree_upgrade, policy_subject_from_label,
    read_bootstrap_state, ready_agent_schedule_nodes, record_child_result_to_parent_context,
    render_agent_system_prompt, reset_unit_for, run_core_tool_cli_with_root,
    set_user_systemd_client_env, skill_metadata_budget_from_env, terminal_command,
    trajectory_from_session_dir, unit_main_pid_for, update_storage_generation_with_prune,
    validate_child_context_lease, validate_trajectory,
};
use serde::Deserialize;

pub(crate) use agent::*;
pub(crate) use attach::*;
pub(crate) use basic::*;
pub(crate) use check::*;
pub(crate) use cortexfs::cli::json;
pub(crate) use cortexfs::cli::stderr;
pub(crate) use cortexfs::cli::terminal::*;
pub(crate) use cortexfs::cli::uid;
pub(crate) use cortexfs::support::plain::{create_exclusive_file_at, open_plain_directory};
#[cfg(test)]
pub(crate) use cortexfs::{agent_host_mount_source, cli::stale::*};
pub(crate) use create::*;
pub(crate) use default::*;
pub(crate) use doctor::*;
pub(crate) use format::*;
pub(crate) use json::*;
pub(crate) use objects::*;
pub(crate) use output::*;
pub(crate) use parse::*;
pub(crate) use procfd::*;
pub(crate) use provider::*;
pub(crate) use schedule::*;
pub(crate) use stderr::*;
pub(crate) use storage::*;
pub(crate) use terminal::*;
pub(crate) use uid::*;
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

pub mod parse;

pub mod agent;

pub mod attach;

pub mod default;

pub mod output;

pub(crate) use cortexfs::cli::create;
pub(crate) use cortexfs::cli::text;
pub(crate) use text::read_small_plain_text_file;

pub(crate) use cortexfs::cli::procfd;

pub mod objects;

pub mod storage;

pub(crate) mod install;

pub(crate) mod package;

pub(crate) mod residue;

pub mod doctor;

pub mod provider;

pub mod file;
pub use file::{basic, check};

pub mod schedule;

pub mod format;

pub mod util;

pub mod terminal;
#[cfg(test)]
#[expect(unused_qualifications, reason = "tests use qualified paths")]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/ctx_tests.rs"
    ));
}
