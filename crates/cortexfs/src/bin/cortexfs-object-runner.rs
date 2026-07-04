#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
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

include!("../object/runner_provider.rs");
include!("object_runner/model.rs");
include!("object_runner/agent.rs");
include!("object_runner/agent_policy.rs");
include!("object_runner/agent_model.rs");
include!("object_runner/agent_io.rs");
include!("object_runner/agent_frames.rs");
include!("object_runner/tool_call.rs");
include!("object_runner/tool_args.rs");
include!("object_runner/tool_exec.rs");
include!("object_runner/timeout.rs");
include!("object_runner/tool.rs");
include!("object_runner/output.rs");
include!("object_runner/fs.rs");
include!("object_runner/exec_path.rs");

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = write_error(&format!("cortexfs-object-runner: {error}"));
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), String> {
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
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/cortexfs_object_runner_tests.rs"
    ));
}
