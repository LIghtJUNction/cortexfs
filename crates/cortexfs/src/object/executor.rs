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

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{
    DEFAULT_AGENT_PROMPT_TEMPLATE, PolicyObjectClass, PolicyPermission, PolicyV0,
    ToolExecutionAuthority, ToolExecutionDenial, authorize_tool_execution, collect_agent_rules,
    collect_skill_metadata, current_time_unix, derive_agent_runtime_view,
    inspect_event_stream_jsonl, is_model_name, is_object_name, parse_model_fallback, run_core_tool,
    run_core_tool_cli, run_echo_model, skill_metadata_budget_from_env, write_run_snapshot,
};
use cortexfs_tool_sdk::ToolInvocation;
use nix::libc;
use serde_json::Value;
use sha2::{Digest, Sha256};

const DEFAULT_SOURCE: &str = "/var/lib/cortexfs/storage/v1-root";
pub(crate) const DEFAULT_CTX_ROOT: &str = "/ctx";
const MAX_AGENT_TOOL_ITERATIONS: usize = 8;
const MAX_MODEL_FALLBACK_CANDIDATES: usize = 16;
const MAX_TOOL_RESULT_CHARS: usize = 16 * 1024;
pub(crate) const MAX_CHILD_STDERR_BYTES: usize = 64 * 1024;
pub(crate) const MAX_STREAM_TOOL_CALL_BUFFER_BYTES: usize = 64 * 1024;
const MAX_AGENT_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_AGENT_TOOL_CONTEXT_BYTES: usize = 64 * 1024;
const MAX_AGENT_TOOL_ARGC: usize = 64;
const MAX_AGENT_TOOL_ARG_BYTES: usize = 8 * 1024;
const MAX_AGENT_MODEL_FRAME_BYTES: usize = 256 * 1024;
const MAX_AGENT_MODEL_FRAMES: usize = 1024;
const MAX_RUNNER_STDIN_INPUT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_RUNNER_CONTROL_BYTES: u64 = 64 * 1024;
const AGENT_TOOL_TIMEOUT_SECONDS: u64 = 20;
const MAX_AGENT_TOOL_TIMEOUT_SECONDS: u64 = 120;
const AGENT_TOOL_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const AGENT_MODEL_TIMEOUT_SECONDS: u64 = 120;
const MAX_AGENT_MODEL_TIMEOUT_SECONDS: u64 = 600;
const BWRAP_PROGRAM: &str = "/usr/bin/bwrap";

pub(crate) use crate::process_helpers;
pub(crate) use process_helpers::read_limited_bytes;

pub(crate) mod agent;
pub(crate) mod frames;
pub(crate) use frames as agent_frames;
pub(crate) mod wire;
pub(crate) use wire as agent_io;
pub(crate) mod inference;
pub(crate) use inference as agent_model;
pub(crate) mod policy;
pub(crate) use policy as agent_policy;
pub(crate) mod model;
pub(crate) use super::runner as runner_provider;
pub(crate) mod call;
use crate::parse_shell_words;
pub(crate) use call as tool_call;
pub(crate) mod path;
pub(crate) use crate::cli::alias as model_alias;
pub(crate) use crate::cli::input as limited_input;
pub(crate) use crate::cli::json;
pub(crate) use crate::cli::nofollow as no_follow_fs;
pub(crate) use path as exec_path;
pub(crate) mod output;
pub(crate) use crate::cli::procfd as proc_fd;
pub(crate) use crate::policy::subject as policy_subject;
pub(crate) mod access;
pub(crate) use crate::cli::stderr;
pub(crate) use crate::cli::text as small_text;
pub(crate) use access as runner_fs;
pub(crate) mod args;
pub(crate) mod timeout;
pub(crate) mod tool;
pub(crate) use args as tool_args;
pub(crate) mod exec;
pub(crate) use agent::*;
pub(crate) use agent_frames::*;
pub(crate) use agent_io::*;
pub(crate) use agent_model::*;
pub(crate) use agent_policy::*;
pub(crate) use exec as tool_exec;
pub(crate) use exec_path::*;
pub(crate) use json::*;
pub(crate) use limited_input::*;
pub(crate) use model::*;
pub(crate) use model_alias::*;
pub(crate) use no_follow_fs::*;
pub(crate) use output::*;
pub(crate) use policy_subject::*;
pub(crate) use proc_fd::*;
pub(crate) use process_helpers::*;
pub(crate) use runner_fs::*;
pub(crate) use runner_provider::*;
pub(crate) use small_text::*;
pub(crate) use stderr::*;
pub(crate) use timeout::*;
pub(crate) use tool::*;
pub(crate) use tool_args::*;
pub(crate) use tool_call::*;
pub(crate) use tool_exec::*;

pub(crate) use crate::plain_fs::open_plain_directory;
pub(crate) use serde::Deserialize;
pub(crate) use std::fmt::Write as _;

#[must_use]
pub(crate) fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = write_error(&format!("cortexfs-object-runner: {error}"));
            ExitCode::from(2)
        }
    }
}

pub(crate) fn run(args: Vec<OsString>) -> Result<(), String> {
    let (object_path, input) = split_object_args(args)?;
    let object = ObjectPath::parse(&object_path)?;
    match (object.class.as_str(), object.name.as_str()) {
        ("model", name) => run_model(name, &input),
        ("agent", name) => run_agent(name, &input),
        ("tool", name) => run_tool(name, &input),
        (class, _name) => Err(format!(
            "object class {class} is not handled by this runner"
        )),
    }
}

#[cfg(test)]
#[expect(unused_qualifications, reason = "tests use qualified paths")]
mod tests;
