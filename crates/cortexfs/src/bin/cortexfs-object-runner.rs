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
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cortexfs::{
    DEFAULT_AGENT_PROMPT_TEMPLATE, PolicyObjectClass, PolicyPermission, PolicyV0,
    ToolExecutionAuthority, ToolExecutionDenial, authorize_tool_execution, collect_agent_rules,
    collect_skill_metadata, current_time_unix, derive_agent_runtime_view,
    inspect_event_stream_jsonl, is_model_name, is_object_name, parse_model_fallback, run_core_tool,
    run_core_tool_cli, run_echo_model, skill_metadata_budget_from_env,
};
use cortexfs_tool_sdk::ToolInvocation;
use nix::libc;
use serde_json::Value;
use sha2::{Digest, Sha256};

const DEFAULT_SOURCE: &str = "/var/lib/cortexfs/storage/v1-root";
const DEFAULT_CTX_ROOT: &str = "/ctx";
const MAX_AGENT_TOOL_ITERATIONS: usize = 8;
const MAX_MODEL_FALLBACK_CANDIDATES: usize = 16;
const MAX_TOOL_RESULT_CHARS: usize = 16 * 1024;
const MAX_CHILD_STDERR_BYTES: usize = 64 * 1024;
const MAX_STREAM_TOOL_CALL_BUFFER_BYTES: usize = 64 * 1024;
const MAX_AGENT_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_AGENT_TOOL_CONTEXT_BYTES: usize = 64 * 1024;
const MAX_AGENT_TOOL_ARGC: usize = 64;
const MAX_AGENT_TOOL_ARG_BYTES: usize = 8 * 1024;
const MAX_AGENT_MODEL_FRAME_BYTES: usize = 256 * 1024;
const MAX_AGENT_MODEL_FRAMES: usize = 1024;
const MAX_RUNNER_STDIN_INPUT_BYTES: usize = 1024 * 1024;
const MAX_RUNNER_CONTROL_BYTES: u64 = 64 * 1024;
const AGENT_TOOL_TIMEOUT_SECONDS: u64 = 20;
const MAX_AGENT_TOOL_TIMEOUT_SECONDS: u64 = 120;
const AGENT_TOOL_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const AGENT_MODEL_TIMEOUT_SECONDS: u64 = 120;
const MAX_AGENT_MODEL_TIMEOUT_SECONDS: u64 = 600;
const BWRAP_PROGRAM: &str = "/usr/bin/bwrap";

#[path = "../support/process-helpers.rs"]
pub mod process_helpers;
use process_helpers::read_limited_bytes;

#[path = "object-runner/agent.rs"]
pub mod agent;
#[path = "object-runner/agent-frames.rs"]
pub mod agent_frames;
#[path = "object-runner/agent-io.rs"]
pub mod agent_io;
#[path = "object-runner/agent-model.rs"]
pub mod agent_model;
#[path = "object-runner/agent-policy.rs"]
pub mod agent_policy;
#[path = "object-runner/model.rs"]
pub mod model;
#[path = "../object/runner-provider.rs"]
pub mod runner_provider;
#[path = "object-runner/tool-call.rs"]
pub mod tool_call;
#[macro_use]
#[path = "shared/shell-words.rs"]
pub mod shell_words;
#[path = "object-runner/exec-path.rs"]
pub mod exec_path;
#[path = "shared/json.rs"]
pub mod json;
#[path = "shared/limited-input.rs"]
pub mod limited_input;
#[path = "shared/model-alias.rs"]
pub mod model_alias;
#[path = "shared/no-follow-fs.rs"]
pub mod no_follow_fs;
#[path = "object-runner/output.rs"]
pub mod output;
#[path = "shared/plain-dir.rs"]
pub mod plain_dir;
#[path = "../policy/subject.rs"]
pub mod policy_subject;
#[path = "shared/proc-fd.rs"]
pub mod proc_fd;
#[path = "object-runner/fs.rs"]
pub mod runner_fs;
#[path = "shared/small-text.rs"]
pub mod small_text;
#[path = "shared/stderr.rs"]
pub mod stderr;
#[path = "object-runner/timeout.rs"]
pub mod timeout;
#[path = "object-runner/tool.rs"]
pub mod tool;
#[path = "object-runner/tool-args.rs"]
pub mod tool_args;
#[path = "object-runner/tool-exec.rs"]
pub mod tool_exec;
pub(crate) use agent::*;
pub(crate) use agent_frames::*;
pub(crate) use agent_io::*;
pub(crate) use agent_model::*;
pub(crate) use agent_policy::*;
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

pub(crate) use cortexfs::plain_fs::open_plain_directory;
pub(crate) use serde::Deserialize;
pub(crate) use std::fmt::Write as _;

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
#[path = "cortexfs-object-runner/tests.rs"]
mod tests;
