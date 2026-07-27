//! SDK for implementing a `CortexFS` executable agent.
//!
//! `CortexFS` invokes an agent executable with one hosted envelope on stdin and
//! runtime context in `CTX_*` environment variables. The agent writes canonical
//! event objects as JSONL to stdout. [`run_cli`] performs the startup ping before
//! invoking agent logic; incomplete capability state fails closed.
//! This crate deliberately exposes no dynamic-library ABI because the
//! runtime executes agent files.

use serde_json::{Value, json};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;

pub use cortexfs_runtime_client::agent::AgentToolObservation;
use cortexfs_runtime_client::agent::{AGENT_ENVELOPE_ARG, AGENT_LAUNCH_ABI, read_agent_invocation};

/// Maximum number of arguments supported by a single tool call.
///
/// Keeping this explicit avoids argv vector overrun in child processes.
const MAX_AGENT_TOOL_ARGC: usize = 64;
/// Maximum total byte budget for serialized tool-call arguments.
///
/// This is enforced before launch to prevent transport fragmentation.
const MAX_AGENT_TOOL_ARG_BYTES: usize = 8 * 1024;
/// Maximum length of stable object identifiers (agent and tool names).
///
/// The value aligns with filesystem-safe identifier conventions used by the runtime.
const MAX_OBJECT_NAME_LEN: usize = 255;
/// Maximum child input size accepted by handoff flow.
///
/// Exceeding this is rejected up-front instead of truncating invocation text.
const MAX_CHILD_INPUT_BYTES: usize = 8 * 1024;

/// Inputs supplied to one executable-agent run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentInvocation {
    /// Run ID from `CTX_RUN_ID`.
    run_id: String,
    /// User input payload.
    input: String,
    /// Agent name from environment binding.
    agent: Option<String>,
    /// Session name from environment binding.
    session: Option<String>,
    /// Optional `CTX_ROOT` binding.
    ctx_root: Option<String>,
    /// Optional `CTX_SOURCE` binding.
    source_root: Option<String>,
    /// Optional historical context for this run.
    history_messages: Option<String>,
    /// Optional tool-context context for continuation.
    tool_context: Option<String>,
    /// Current continuation step.
    step: u8,
    /// Optional prior observation for step > 0.
    observation: Option<AgentToolObservation>,
}

impl AgentInvocation {
    /// Creates an invocation for embedding or tests.
    #[must_use]
    pub fn new(run_id: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            input: input.into(),
            agent: None,
            session: None,
            ctx_root: None,
            source_root: None,
            history_messages: None,
            tool_context: None,
            step: 0,
            observation: None,
        }
    }

    /// Returns the runtime run identifier.
    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
    /// Returns the user input passed by `CortexFS`.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }
    /// Returns the selected agent name, when supplied.
    #[must_use]
    pub fn agent(&self) -> Option<&str> {
        self.agent.as_deref()
    }
    /// Returns the selected durable session, when supplied.
    #[must_use]
    pub fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }
    /// Returns the visible `CortexFS` ABI root, when supplied.
    #[must_use]
    pub fn ctx_root(&self) -> Option<&str> {
        self.ctx_root.as_deref()
    }
    /// Returns the backing source root, when supplied.
    #[must_use]
    pub fn source_root(&self) -> Option<&str> {
        self.source_root.as_deref()
    }
    /// Returns the bounded historical message context, when supplied.
    #[must_use]
    pub fn history_messages(&self) -> Option<&str> {
        self.history_messages.as_deref()
    }
    /// Returns the runtime-owned tool context, when supplied.
    #[must_use]
    pub fn tool_context(&self) -> Option<&str> {
        self.tool_context.as_deref()
    }
    /// Returns the host-owned continuation step.
    #[must_use]
    pub const fn step(&self) -> u8 {
        self.step
    }
    /// Returns the authoritative result from the immediately preceding call.
    #[must_use]
    pub const fn observation(&self) -> Option<&AgentToolObservation> {
        self.observation.as_ref()
    }
}

/// Error returned by custom agent logic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentError {
    /// Stable errno-style code.
    code: String,
    /// Human-readable diagnostic text.
    message: String,
}

impl AgentError {
    /// Creates an error using a stable errno-style code.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    /// Creates an invalid-input error.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("EINVAL", message)
    }
    /// Returns the stable error code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }
    /// Returns the user-visible error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Result returned by custom agent logic.
pub type AgentResult<T> = Result<T, AgentError>;

/// One terminal tool request yielded to the `CortexFS` host executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentToolCallRequest {
    /// Correlation identifier for the host-produced result.
    pub id: String,
    /// `CortexFS` tool object name.
    pub name: String,
    /// UTF-8 argument vector passed to the tool executable.
    pub args: Vec<String>,
}

impl AgentToolCallRequest {
    /// Creates and validates a host-executed tool request.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        args: Vec<String>,
    ) -> AgentResult<Self> {
        let request = Self {
            id: id.into(),
            name: name.into(),
            args,
        };
        request.validate()?;
        Ok(request)
    }

    /// Validates tool call identifiers and argument constraints before use.
    ///
    /// Validation uses a stable object-name rule and keeps invocation payload
    /// bounded to avoid oversized argv or malformed transitions.
    fn validate(&self) -> AgentResult<()> {
        if !is_object_name(&self.id) {
            return Err(AgentError::invalid("invalid tool_call id"));
        }
        if !is_object_name(&self.name) {
            return Err(AgentError::invalid("invalid tool_call name"));
        }
        if self.args.len() > MAX_AGENT_TOOL_ARGC {
            return Err(AgentError::invalid(
                "tool_call args exceed argument count limit",
            ));
        }
        let bytes = self
            .args
            .iter()
            .map(String::len)
            .try_fold(0_usize, usize::checked_add)
            .ok_or_else(|| AgentError::invalid("tool_call args exceed byte limit"))?;
        if bytes > MAX_AGENT_TOOL_ARG_BYTES {
            return Err(AgentError::invalid("tool_call args exceed byte limit"));
        }
        Ok(())
    }
}

/// Terminal outcome of one executable-agent invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentOutcome {
    /// Agent work completed; the host finalizes the run lifecycle.
    Complete,
    /// Control returns to the host for exactly one authoritative tool execution.
    YieldToolCall(AgentToolCallRequest),
}

/// Canonical JSONL event writer for one agent run.
#[derive(Debug)]
pub struct AgentEmitter<W> {
    run_id: String,
    writer: W,
}

impl<W: Write> AgentEmitter<W> {
    /// Creates an emitter bound to one run.
    #[must_use]
    pub fn new(run_id: impl Into<String>, writer: W) -> Self {
        Self {
            run_id: run_id.into(),
            writer,
        }
    }

    /// Emits an incremental assistant text event.
    pub fn delta(&mut self, text: &str) -> io::Result<()> {
        self.frame(&json!({ "type": "delta", "run": self.run_id, "text": text }))
    }

    /// Emits a complete assistant message event.
    pub fn message(&mut self, text: &str) -> io::Result<()> {
        self.frame(&json!({
            "type": "message", "run": self.run_id, "role": "assistant",
            "content": [{ "type": "text", "text": text }]
        }))
    }

    /// Emits a custom canonical event using the current run id.
    ///
    /// Lifecycle and capability event types are reserved for the host.
    pub fn event(&mut self, mut event: Value) -> AgentResult<()> {
        let has_tool_result = event_has_tool_result(&event);
        let object = event
            .as_object_mut()
            .ok_or_else(|| AgentError::invalid("agent event must be a JSON object"))?;
        let event_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::invalid("agent event requires string type"))?;
        if matches!(
            event_type,
            "start"
                | "done"
                | "error"
                | "tool_call"
                | "tool_result"
                | "approval_request"
                | "approval_result"
        ) || has_tool_result
        {
            return Err(AgentError::invalid("agent event type is reserved"));
        }
        object.insert("run".to_owned(), Value::String(self.run_id.clone()));
        self.frame(&event)
            .map_err(|error| AgentError::new("EIO", error.to_string()))
    }

    /// Emits the canonical host event that requests a tool execution.
    ///
    /// The request is revalidated at emit time so the host always receives a
    /// fully safe payload.
    fn tool_call(&mut self, request: &AgentToolCallRequest) -> AgentResult<()> {
        request.validate()?;
        self.frame(&json!({
            "type": "tool_call",
            "run": self.run_id,
            "id": request.id,
            "name": request.name,
            "arguments": { "args": request.args }
        }))
        .map_err(|error| AgentError::new("EIO", error.to_string()))
    }

    /// Writes one JSON frame and flushes it so host readers observe progress
    /// deterministically.
    fn frame(&mut self, value: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, value)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

/// Custom executable-agent logic.
pub trait Agent: fmt::Debug {
    /// Handles one `CortexFS` invocation.
    fn run(
        &self,
        invocation: &AgentInvocation,
        output: &mut AgentEmitter<&mut dyn Write>,
    ) -> AgentResult<AgentOutcome>;
}

/// Runs one invocation and converts the typed outcome into a success boolean.
fn run_agent_status(
    agent: &dyn Agent,
    invocation: &AgentInvocation,
    writer: &mut dyn Write,
) -> bool {
    let mut output = AgentEmitter::new(invocation.run_id().to_owned(), writer);
    match agent.run(invocation, &mut output) {
        Ok(AgentOutcome::Complete) => true,
        Ok(AgentOutcome::YieldToolCall(request)) => output.tool_call(&request).is_ok(),
        Err(_) => false,
    }
}

/// Runs an agent as the executable entry point expected by `CortexFS`.
pub fn run_cli<I>(agent: &dyn Agent, args: I) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    if env::var("CTX_AGENT_LAUNCH").as_deref() != Ok(AGENT_LAUNCH_ABI)
        || args.as_slice() != [OsString::from(AGENT_ENVELOPE_ARG)]
    {
        return ExitCode::from(2);
    }
    let stdin = io::stdin();
    let Ok(envelope) = read_agent_invocation(stdin.lock()) else {
        return ExitCode::from(2);
    };
    let Some(run_id) = env::var_os("CTX_RUN_ID").and_then(|value| value.into_string().ok()) else {
        return ExitCode::from(2);
    };
    let mut invocation = AgentInvocation::new(run_id, envelope.input());
    invocation.agent = env_text("CTX_AGENT");
    invocation.session = env_text("CTX_SESSION");
    invocation.ctx_root = env_text("CTX_ROOT");
    invocation.source_root = env_text("CTX_SOURCE");
    if envelope.run() != invocation.run_id
        || env::var("CTX_AGENT_STEP").ok().as_deref() != Some(&envelope.step().to_string())
    {
        return ExitCode::from(2);
    }
    invocation.history_messages = Some(envelope.history_messages().to_owned());
    invocation.tool_context = Some(envelope.tool_context().to_owned());
    invocation.step = envelope.step();
    invocation.observation = envelope.observation().cloned();
    let Some(agent_name) = invocation.agent() else {
        return ExitCode::from(2);
    };
    if startup_handshake(agent_name).is_err() {
        return ExitCode::from(1);
    }
    let stdout = io::stdout();
    if run_agent_status(agent, &invocation, &mut stdout.lock()) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Checks startup compatibility with runtime capability ping before executing the agent.
fn startup_handshake(agent: &str) -> Result<(), cortexfs_runtime_client::RuntimeClientError> {
    cortexfs_runtime_client::ping_from_environment(agent).map(|_| ())
}

/// Creates an owned child through the receipt-bound runtime capability.
///
/// `path` is an optional attenuated `CTX_PATH`; `None` inherits the parent's
/// path exactly. `window` may only attenuate the inherited context window.
pub fn create_child(
    child: &str,
    child_session: &str,
    path: Option<&str>,
    window: Option<u32>,
    input: &str,
) -> Result<cortexfs_runtime_client::CreateChildResult, cortexfs_runtime_client::RuntimeClientError>
{
    if !is_object_name(child)
        || !is_object_name(child_session)
        || input.contains('\0')
        || input.len() > MAX_CHILD_INPUT_BYTES
        || window == Some(0)
    {
        return Err(cortexfs_runtime_client::RuntimeClientError::InvalidEnvironment);
    }
    let request_id = cortexfs_runtime_client::fresh_request_id("agent-create")?;
    cortexfs_runtime_client::create_child_from_environment(
        cortexfs_runtime_client::CreateChildEnvironmentRequest {
            request_id: &request_id,
            child,
            child_session,
            path,
            window,
            input,
            life: "owned",
        },
    )
}

/// Validates names used for child and tool call identifiers.
///
/// The same naming rule is shared by child creation and tool routing to avoid
/// ambiguous object identifiers.
fn is_object_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > MAX_OBJECT_NAME_LEN
        || name.strip_suffix(".sock").is_some()
        || name.strip_suffix(".d").is_some()
    {
        return false;
    }
    name.bytes().enumerate().all(|(index, byte)| {
        byte.is_ascii_alphanumeric() || (index != 0 && matches!(byte, b'.' | b'_' | b'+' | b'-'))
    })
}

/// Detects whether an event already carries canonical tool-result payload.
///
/// This filter rejects re-encoding mistakes where a tool response is
/// accidentally emitted as free-form agent event content.
fn event_has_tool_result(event: &Value) -> bool {
    event.get("role").and_then(Value::as_str) == Some("tool")
        || event
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts
                    .iter()
                    .any(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
            })
}

/// Reads an environment variable as UTF-8 when present.
///
/// Non-UTF-8 values are ignored to keep environment parsing fail-safe.
fn env_text(name: &str) -> Option<String> {
    env::var_os(name).and_then(|value| value.into_string().ok())
}

/// Defines the executable entry point for an agent value.
#[macro_export]
macro_rules! cortexfs_agent_main {
    ($agent:expr) => {
        fn main() -> std::process::ExitCode {
            $crate::run_cli(&$agent, std::env::args_os().skip(1))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    /// Builds a hosted invocation envelope for subprocess-level tests.
    fn hosted_envelope(run: &str, step: u8) -> String {
        serde_json::json!({
            "schema": cortexfs_runtime_client::agent::AGENT_INVOCATION_SCHEMA,
            "run": run, "step": step,
            "input": "hello", "history_messages": "[]", "tool_context": "",
            "observation": Value::Null
        })
        .to_string()
            + "\n"
    }

    #[test]
    #[ignore = "subprocess entrypoint for rejected hosted CLI tests"]
    fn rejected_cli_subprocess() {
        let arg = env::var_os("TEST_AGENT_ARG").unwrap_or_default();
        assert_eq!(run_cli(&Echo, [arg]), ExitCode::from(2));
    }

    #[test]
    #[ignore = "subprocess entrypoint for child capability test"]
    fn create_child_subprocess() {
        assert!(create_child("worker-a", "child-a", None, None, "first handoff").is_ok());
        assert!(
            create_child(
                "worker-b",
                "child-b",
                Some("/ctx/home/1000/tool"),
                Some(2048),
                "second handoff",
            )
            .is_ok()
        );
    }

    #[test]
    #[ignore = "subprocess entrypoint for hosted envelope test"]
    fn hosted_envelope_subprocess() {
        assert_eq!(
            run_cli(&Echo, [OsString::from(AGENT_ENVELOPE_ARG)]),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn sdk_startup_handshake_reaches_runtime_capability() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let control = root.path().join("control");
        std::fs::create_dir_all(&control)?;
        std::fs::set_permissions(&control, std::fs::Permissions::from_mode(0o711))?;
        let identity = std::fs::metadata(&control)?;
        let (capability, listener) = cortexfs::runtime::control::RunCapability::create(
            &control,
            "echo-agent",
            "live",
            "run-1",
            identity.uid(),
            identity.gid(),
        )?;
        let environment = capability.environment(capability.socket());
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            capability.serve_run(&listener, &server_shutdown, &startup_tx, || {
                Some("run-1".to_owned())
            })
        });
        let mut child = std::process::Command::new(env::current_exe()?)
            .arg("--exact")
            .arg("tests::hosted_envelope_subprocess")
            .arg("--ignored")
            .env("CTX_AGENT", "echo-agent")
            .env("CTX_SESSION", "live")
            .env("CTX_RUN_ID", "run-1")
            .env("CTX_AGENT_LAUNCH", AGENT_LAUNCH_ABI)
            .env("CTX_AGENT_STEP", "0")
            .env(&environment[0].0, &environment[0].1)
            .env(&environment[1].0, &environment[1].1)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let Some(mut stdin) = child.stdin.take() else {
            return Err(io::Error::other("child stdin unavailable").into());
        };
        stdin.write_all(hosted_envelope("run-1", 0).as_bytes())?;
        drop(stdin);
        let output = child.wait_with_output()?;
        assert!(output.status.success(), "{output:?}");
        assert!(matches!(
            startup_rx.recv_timeout(std::time::Duration::from_secs(1)),
            Ok(Ok(()))
        ));
        shutdown.store(true, Ordering::Release);
        assert!(matches!(server.join(), Ok(Ok(()))));
        Ok(())
    }

    #[test]
    fn sdk_create_child_uses_capability_and_fresh_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let control = root.path().join("control");
        std::fs::create_dir_all(&control)?;
        std::fs::set_permissions(&control, std::fs::Permissions::from_mode(0o711))?;
        let identity = std::fs::metadata(&control)?;
        let (capability, listener) = cortexfs::runtime::control::RunCapability::create(
            &control,
            "parent",
            "default",
            "run-1",
            identity.uid(),
            identity.gid(),
        )?;
        let environment = capability.environment(capability.socket());
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let (startup_tx, _startup_rx) = mpsc::sync_channel(1);
        let (handoff_tx, handoff_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            capability.serve_run_with_handler(
                &listener,
                &server_shutdown,
                &startup_tx,
                || Some("run-1".to_owned()),
                |request| {
                    handoff_tx
                        .send((
                            request.child.clone(),
                            request.path.clone(),
                            request.window,
                            request.input.clone(),
                            request.life.clone(),
                        ))
                        .map_err(|_error| {
                            cortexfs::runtime::control::RunCapabilityError::CannotCreate
                        })?;
                    Ok(cortexfs::runtime::control::CreateChildResult {
                        child: request.child,
                        child_session: request.child_session,
                        pid: 42,
                    })
                },
                |_request| Err(cortexfs::runtime::control::RunCapabilityError::Unsupported),
            )
        });
        let output = std::process::Command::new(env::current_exe()?)
            .arg("--exact")
            .arg("tests::create_child_subprocess")
            .arg("--ignored")
            .env("CTX_AGENT", "parent")
            .env("CTX_SESSION", "default")
            .env("CTX_RUN_ID", "run-1")
            .env(&environment[0].0, &environment[0].1)
            .env(&environment[1].0, &environment[1].1)
            .output()?;
        assert!(output.status.success(), "{output:?}");
        let handoffs = [handoff_rx.recv()?, handoff_rx.recv()?];
        assert_eq!(
            handoffs,
            [
                (
                    "worker-a".to_owned(),
                    None,
                    None,
                    "first handoff".to_owned(),
                    "owned".to_owned()
                ),
                (
                    "worker-b".to_owned(),
                    Some("/ctx/home/1000/tool".to_owned()),
                    Some(2048),
                    "second handoff".to_owned(),
                    "owned".to_owned()
                )
            ]
        );
        shutdown.store(true, Ordering::Release);
        assert!(matches!(server.join(), Ok(Ok(()))));
        Ok(())
    }

    #[test]
    fn sdk_requires_marker_and_launch_env_independently() -> Result<(), Box<dyn std::error::Error>>
    {
        for (arg, launch) in [
            ("wrong-marker", Some(AGENT_LAUNCH_ABI)),
            (AGENT_ENVELOPE_ARG, None),
            (AGENT_ENVELOPE_ARG, Some("wrong-abi")),
        ] {
            let mut command = std::process::Command::new(env::current_exe()?);
            command
                .arg("--exact")
                .arg("tests::rejected_cli_subprocess")
                .arg("--ignored")
                .env("TEST_AGENT_ARG", arg)
                .env("CTX_AGENT", "echo-agent")
                .env("CTX_RUN_ID", "run-1")
                .env_remove("CTX_CONTROL_SOCKET")
                .env_remove("CTX_CONTROL_TOKEN");
            if let Some(launch) = launch {
                command.env("CTX_AGENT_LAUNCH", launch);
            } else {
                command.env_remove("CTX_AGENT_LAUNCH");
            }
            let output = command.output()?;
            assert!(output.status.success(), "{arg} {launch:?}: {output:?}");
        }
        Ok(())
    }

    #[test]
    fn sdk_rejects_zero_child_window_before_connect() {
        assert_eq!(
            create_child("worker", "child", None, Some(0), "handoff"),
            Err(cortexfs_runtime_client::RuntimeClientError::InvalidEnvironment)
        );
    }

    #[derive(Debug)]
    struct Echo;
    impl Agent for Echo {
        fn run(
            &self,
            invocation: &AgentInvocation,
            output: &mut AgentEmitter<&mut dyn Write>,
        ) -> AgentResult<AgentOutcome> {
            output
                .message(invocation.input())
                .map_err(|error| AgentError::new("EIO", error.to_string()))?;
            Ok(AgentOutcome::Complete)
        }
    }

    #[test]
    fn agent_error_returns_failure_without_host_frames() {
        #[derive(Debug)]
        struct Fail;
        impl Agent for Fail {
            fn run(
                &self,
                _invocation: &AgentInvocation,
                _output: &mut AgentEmitter<&mut dyn Write>,
            ) -> AgentResult<AgentOutcome> {
                Err(AgentError::invalid("bad input"))
            }
        }

        let mut bytes = Vec::new();
        assert!(!run_agent_status(
            &Fail,
            &AgentInvocation::new("r1", ""),
            &mut bytes,
        ));
        assert!(bytes.is_empty());
    }

    #[test]
    fn event_requires_object_and_type() {
        let mut bytes = Vec::new();
        let mut emitter = AgentEmitter::new("r1", &mut bytes);
        assert!(emitter.event(Value::Null).is_err());
        assert!(emitter.event(json!({})).is_err());
    }

    #[test]
    fn event_forces_current_run_and_rejects_lifecycle_types() {
        let mut bytes = Vec::new();
        let mut emitter = AgentEmitter::new("current", &mut bytes);
        assert!(
            emitter
                .event(json!({ "type": "usage", "run": "spoofed" }))
                .is_ok()
        );
        for event_type in [
            "start",
            "done",
            "error",
            "tool_call",
            "tool_result",
            "approval_request",
            "approval_result",
        ] {
            let error = emitter.event(json!({ "type": event_type }));
            assert!(matches!(error, Err(ref error) if error.code() == "EINVAL"));
        }
        let frame: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        assert_eq!(frame, json!({ "type": "usage", "run": "current" }));
    }

    #[test]
    fn tool_yield_emits_only_the_host_request() {
        #[derive(Debug)]
        struct Yield;
        impl Agent for Yield {
            fn run(
                &self,
                _invocation: &AgentInvocation,
                _output: &mut AgentEmitter<&mut dyn Write>,
            ) -> AgentResult<AgentOutcome> {
                AgentToolCallRequest::new("call-1", "example.echo", vec!["hello".to_owned()])
                    .map(AgentOutcome::YieldToolCall)
            }
        }

        let mut bytes = Vec::new();
        assert!(run_agent_status(
            &Yield,
            &AgentInvocation::new("r1", ""),
            &mut bytes,
        ));
        let frames = String::from_utf8(bytes).unwrap_or_default();
        let types = frames
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|frame| frame.get("type")?.as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(types, ["tool_call"]);
    }

    #[test]
    fn tool_request_enforces_host_argument_limits() {
        assert!(AgentToolCallRequest::new("bad/name", "example.echo", Vec::new()).is_err());
        assert!(
            AgentToolCallRequest::new(
                "call-1",
                "example.echo",
                vec![String::new(); MAX_AGENT_TOOL_ARGC.saturating_add(1)],
            )
            .is_err()
        );
        assert!(
            AgentToolCallRequest::new(
                "call-1",
                "example.echo",
                vec!["x".repeat(MAX_AGENT_TOOL_ARG_BYTES.saturating_add(1))],
            )
            .is_err()
        );
    }

    #[test]
    fn hosted_envelope_is_strict_and_typed() -> Result<(), Box<dyn std::error::Error>> {
        let input = br#"{"schema":"cortexfs.agent-invocation/v1","run":"r1","step":1,"input":"hello","history_messages":"[]","tool_context":"","observation":{"tool_call_id":"call-1","name":"example.echo","status":"ok","content":"a","truncated":false}}
"#;
        let envelope = read_agent_invocation(Cursor::new(input))
            .map_err(|error| io::Error::other(format!("agent SDK test failure: {error:?}")))?;
        assert_eq!(envelope.step(), 1);
        assert_eq!(
            envelope.observation().map(AgentToolObservation::content),
            Some("a")
        );
        let body = input
            .get(..input.len().saturating_sub(1))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing envelope body"))?;
        for invalid in [body, b"{}\n".as_slice(), b"{}\n{}\n".as_slice()] {
            assert!(read_agent_invocation(Cursor::new(invalid)).is_err());
        }
        assert!(read_agent_invocation(Cursor::new(vec![0xff, b'\n'])).is_err());
        let base: Value = serde_json::from_slice(body)?;
        for (field, value) in [
            ("schema", json!("wrong/v1")),
            ("step", json!(9)),
            ("observation", Value::Null),
            (
                "history_messages",
                json!("x".repeat(cortexfs_runtime_client::agent::MAX_AGENT_CONTEXT_BYTES + 1)),
            ),
        ] {
            let mut invalid = base.clone();
            invalid
                .as_object_mut()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid envelope"))?
                .insert(field.to_owned(), value);
            let bytes = invalid.to_string() + "\n";
            assert!(
                read_agent_invocation(Cursor::new(bytes)).is_err(),
                "{field}"
            );
        }
        let mut zero_with_observation = base;
        let mut oversized_observation = zero_with_observation.clone();
        oversized_observation
            .get_mut("observation")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing observation"))?
            .insert("content".to_owned(), json!("x".repeat(16 * 1024 + 1)));
        let bytes = oversized_observation.to_string() + "\n";
        assert!(read_agent_invocation(Cursor::new(bytes)).is_err());
        zero_with_observation
            .as_object_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid envelope"))?
            .insert("step".to_owned(), json!(0));
        let bytes = zero_with_observation.to_string() + "\n";
        assert!(read_agent_invocation(Cursor::new(bytes)).is_err());
        Ok(())
    }

    #[test]
    fn hosted_envelope_validates_run_and_step_without_lifecycle()
    -> Result<(), Box<dyn std::error::Error>> {
        let binary = env::current_exe()?;
        for (run, env_step, success) in
            [("r1", "0", true), ("wrong", "0", false), ("r1", "1", false)]
        {
            let mut child = std::process::Command::new(&binary)
                .arg("--exact")
                .arg("tests::hosted_envelope_subprocess")
                .arg("--ignored")
                .env("CTX_AGENT", "echo-agent")
                .env("CTX_SESSION", "default")
                .env("CTX_RUN_ID", "r1")
                .env("CTX_AGENT_LAUNCH", AGENT_LAUNCH_ABI)
                .env("CTX_AGENT_STEP", env_step)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()?;
            let Some(mut stdin) = child.stdin.take() else {
                return Err(io::Error::other("child stdin unavailable").into());
            };
            stdin.write_all(hosted_envelope(run, 0).as_bytes())?;
            drop(stdin);
            let output = child.wait_with_output()?;
            assert_eq!(output.status.success(), success, "{output:?}");
            if success {
                let text = String::from_utf8_lossy(&output.stdout);
                assert_eq!(text.matches("\"type\":\"start\"").count(), 0);
                assert_eq!(text.matches("\"type\":\"done\"").count(), 0);
                assert_eq!(text.matches("\"type\":\"message\"").count(), 1);
            }
        }
        Ok(())
    }
}
