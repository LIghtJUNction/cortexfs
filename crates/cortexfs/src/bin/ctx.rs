use std::env;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use cortexfs::{
    AbiPathKind, AgentControlIssue, AgentPromptContext, AgentRuntimeView, CTX_ROOT,
    ContextJsonlIssue, ContextPackIssue, DEFAULT_AGENT_PROMPT_TEMPLATE, EventStreamIssue,
    MAX_SOCKET_FRAME_BYTES, MessageStreamIssue, ModelCapabilityIssue, ModelDriverRouteError,
    MountMode, MountTable, ObjectClass, ObjectLayoutIssue, PolicyV0, ROOT_ENTRIES,
    SessionControlIssue, SessionIndexIssue, SessionIndexKind, SessionLayoutIssue,
    SharedQueueLayoutIssue, ToolPath, ToolSchemaIssue, classify_abi_path,
    derive_agent_runtime_view, ensure_v1_reference_tree, inspect_agent_control,
    inspect_context_jsonl, inspect_context_pack_json, inspect_event_stream_jsonl,
    inspect_message_stream_jsonl, inspect_model_capabilities, inspect_object_layout,
    inspect_session_control, inspect_session_index, inspect_session_layout,
    inspect_shared_queue_layout, inspect_tool_schema_json, is_executable_file, is_model_name,
    is_object_name, parse_abi_path, parse_model_driver_routes, render_agent_system_prompt,
};
use nix::sys::termios::{SetArg, Termios, cfmakeraw, tcgetattr, tcsetattr};

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            let _ignored = write_error(&format!("ctx: {}", error.message));
            ExitCode::from(error.code)
        }
    }
}

include!("ctx/parse.rs");

include!("ctx/agent.rs");

include!("ctx/output_mount.rs");

include!("ctx/objects_socket.rs");

include!("ctx/doctor.rs");

include!("ctx/file_basic.rs");

include!("ctx/file_check.rs");

include!("ctx/format.rs");

include!("ctx/util.rs");

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/ctx_tests.rs"
    ));
}
