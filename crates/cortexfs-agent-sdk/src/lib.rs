//! SDK for implementing a `CortexFS` executable agent.
//!
//! `CortexFS` invokes an agent executable with user input in CLI arguments or
//! stdin and runtime context in `CTX_*` environment variables. [`run_cli`]
//! joins argument values with spaces; when no argument is present, it reads up
//! to 1 MiB of stdin. The agent writes canonical event objects as JSONL to
//! stdout. When the runtime supplies a complete `CTX_CONTROL_*` capability,
//! [`run_cli`] performs the startup ping before emitting `start`; absent
//! capability state permits standalone use and partial state fails closed.
//! This crate deliberately exposes no dynamic-library ABI because the
//! runtime executes agent files.

use serde_json::{Value, json};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::process::ExitCode;

/// Maximum input accepted from stdin by [`run_cli`].
pub const MAX_CLI_STDIN_INPUT_BYTES: usize = 1024 * 1024;
const HOSTED_ENVELOPE_ARG: &str = "--cortexfs-sdk-envelope-v1";
const HOSTED_ENVELOPE_ENV: &str = "sdk-envelope-v1";
const AGENT_INVOCATION_SCHEMA: &str = "cortexfs.agent-invocation/v1";
const MAX_AGENT_CONTEXT_BYTES: usize = 64 * 1024;
const MAX_AGENT_STEPS: u8 = 8;
const MAX_AGENT_TOOL_ARGC: usize = 64;
const MAX_AGENT_TOOL_ARG_BYTES: usize = 8 * 1024;
const MAX_OBJECT_NAME_LEN: usize = 255;
const MAX_CHILD_INPUT_BYTES: usize = 8 * 1024;

/// Inputs supplied to one executable-agent run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentInvocation {
    run_id: String,
    input: String,
    agent: Option<String>,
    session: Option<String>,
    ctx_root: Option<String>,
    source_root: Option<String>,
    history_messages: Option<String>,
    tool_context: Option<String>,
    step: u8,
    observation: Option<AgentToolObservation>,
    hosted: bool,
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
            hosted: false,
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

/// Authoritative host observation supplied to one continuation step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentToolObservation {
    tool_call_id: String,
    name: String,
    status: String,
    content: String,
    truncated: bool,
}

impl AgentToolObservation {
    /// Returns the preceding call identifier.
    #[must_use]
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }
    /// Returns the executed tool name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns `ok` or `error`.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }
    /// Returns the normalized authoritative result.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
    /// Returns whether normalization truncated the result.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

/// Error returned by custom agent logic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentError {
    code: String,
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
    /// Agent work completed and the SDK must emit the final `done` frame.
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
    /// Lifecycle event types are reserved for [`run_agent`].
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
            "start" | "done" | "error" | "tool_call" | "tool_result"
        ) || has_tool_result
        {
            return Err(AgentError::invalid("agent event type is reserved"));
        }
        object.insert("run".to_owned(), Value::String(self.run_id.clone()));
        self.frame(&event)
            .map_err(|error| AgentError::new("EIO", error.to_string()))
    }

    fn error(&mut self, error: &AgentError) -> io::Result<()> {
        self.frame(&json!({ "type": "error", "run": self.run_id, "code": error.code, "message": error.message }))
    }

    fn done(&mut self, status: &str) -> io::Result<()> {
        self.frame(&json!({ "type": "done", "run": self.run_id, "status": status }))
    }

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

/// Runs custom agent logic and completes the canonical event stream.
pub fn run_agent(
    agent: &dyn Agent,
    invocation: &AgentInvocation,
    writer: &mut dyn Write,
) -> io::Result<()> {
    run_agent_status(agent, invocation, writer, true).map(|_success| ())
}

fn run_agent_status(
    agent: &dyn Agent,
    invocation: &AgentInvocation,
    writer: &mut dyn Write,
    lifecycle: bool,
) -> io::Result<bool> {
    let mut output = AgentEmitter::new(invocation.run_id().to_owned(), writer);
    if lifecycle {
        output.frame(&json!({ "type": "start", "run": invocation.run_id() }))?;
    }
    match agent.run(invocation, &mut output) {
        Ok(AgentOutcome::Complete) => {
            if lifecycle {
                output.done("ok")?;
            }
            Ok(true)
        }
        Ok(AgentOutcome::YieldToolCall(request)) => {
            if let Err(error) = output.tool_call(&request) {
                if lifecycle {
                    output.error(&error)?;
                    output.done("error")?;
                }
                return Ok(false);
            }
            Ok(true)
        }
        Err(error) => {
            if lifecycle {
                output.error(&error)?;
                output.done("error")?;
            }
            Ok(false)
        }
    }
}

/// Runs an agent as the executable entry point expected by `CortexFS`.
pub fn run_cli<I>(agent: &dyn Agent, args: I) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let hosted = env::var("CTX_AGENT_LAUNCH").as_deref() == Ok(HOSTED_ENVELOPE_ENV)
        && args.as_slice() == [OsString::from(HOSTED_ENVELOPE_ARG)];
    let stdin = io::stdin();
    let envelope = if hosted {
        parse_hosted_envelope(stdin.lock()).ok()
    } else {
        None
    };
    if hosted && envelope.is_none() {
        return ExitCode::from(2);
    }
    let input = match envelope.as_ref() {
        Some(envelope) => envelope.input.clone(),
        None => match collect_input_from_reader(args, stdin.lock()) {
            Ok(input) => input,
            Err(_) => return ExitCode::from(2),
        },
    };
    let Some(run_id) = env::var_os("CTX_RUN_ID").and_then(|value| value.into_string().ok()) else {
        return ExitCode::from(2);
    };
    let mut invocation = AgentInvocation::new(run_id, input);
    invocation.agent = env_text("CTX_AGENT");
    invocation.session = env_text("CTX_SESSION");
    invocation.ctx_root = env_text("CTX_ROOT");
    invocation.source_root = env_text("CTX_SOURCE");
    if let Some(envelope) = envelope {
        if envelope.run != invocation.run_id
            || env::var("CTX_AGENT_STEP").ok().as_deref() != Some(&envelope.step.to_string())
        {
            return ExitCode::from(2);
        }
        invocation.history_messages = Some(envelope.history_messages);
        invocation.tool_context = Some(envelope.tool_context);
        invocation.step = envelope.step;
        invocation.observation = envelope.observation;
        invocation.hosted = true;
    } else {
        invocation.history_messages = env_text("CTX_AGENT_HISTORY_MESSAGES");
        invocation.tool_context = env_text("CTX_AGENT_TOOL_CONTEXT");
    }
    let Some(agent_name) = invocation.agent() else {
        return ExitCode::from(2);
    };
    if startup_handshake(agent_name).is_err() {
        return ExitCode::from(1);
    }
    let stdout = io::stdout();
    match run_agent_status(agent, &invocation, &mut stdout.lock(), !invocation.hosted) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) | Err(_) => ExitCode::from(1),
    }
}

struct HostedEnvelope {
    run: String,
    step: u8,
    input: String,
    history_messages: String,
    tool_context: String,
    observation: Option<AgentToolObservation>,
}

fn parse_hosted_envelope(reader: impl Read) -> AgentResult<HostedEnvelope> {
    let limit = u64::try_from(MAX_CLI_STDIN_INPUT_BYTES.saturating_add(2))
        .map_err(|_error| AgentError::invalid("hosted invocation input limit is invalid"))?;
    let mut bytes = Vec::new();
    reader
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| AgentError::new("EIO", error.to_string()))?;
    if bytes.len() > MAX_CLI_STDIN_INPUT_BYTES
        || bytes.last() != Some(&b'\n')
        || bytes
            .iter()
            .take(bytes.len().saturating_sub(1))
            .any(|byte| *byte == b'\n')
    {
        return Err(AgentError::invalid("invalid hosted invocation framing"));
    }
    bytes.pop();
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_error| AgentError::invalid("invalid hosted invocation JSON"))?;
    let object = exact_object(
        &value,
        &[
            "schema",
            "run",
            "step",
            "input",
            "history_messages",
            "tool_context",
            "observation",
        ],
    )?;
    if object.get("schema").and_then(Value::as_str) != Some(AGENT_INVOCATION_SCHEMA) {
        return Err(AgentError::invalid("invalid hosted invocation schema"));
    }
    let run = required_string(object, "run")?;
    let step = object
        .get("step")
        .and_then(Value::as_u64)
        .and_then(|v| u8::try_from(v).ok())
        .filter(|step| *step <= MAX_AGENT_STEPS)
        .ok_or_else(|| AgentError::invalid("invalid hosted invocation step"))?;
    let input = required_string(object, "input")?;
    let history_messages = required_string(object, "history_messages")?;
    let tool_context = required_string(object, "tool_context")?;
    if history_messages.len() > MAX_AGENT_CONTEXT_BYTES
        || tool_context.len() > MAX_AGENT_CONTEXT_BYTES
    {
        return Err(AgentError::invalid(
            "hosted invocation context exceeds limit",
        ));
    }
    let observation = parse_observation(object.get("observation"))?;
    if (step == 0) != observation.is_none() {
        return Err(AgentError::invalid(
            "hosted invocation observation cardinality",
        ));
    }
    Ok(HostedEnvelope {
        run,
        step,
        input,
        history_messages,
        tool_context,
        observation,
    })
}

fn parse_observation(value: Option<&Value>) -> AgentResult<Option<AgentToolObservation>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let object = exact_object(
        value,
        &["tool_call_id", "name", "status", "content", "truncated"],
    )?;
    let tool_call_id = required_string(object, "tool_call_id")?;
    let name = required_string(object, "name")?;
    if !is_object_name(&tool_call_id) || !is_object_name(&name) {
        return Err(AgentError::invalid("invalid observation identity"));
    }
    let status = required_string(object, "status")?;
    if !matches!(status.as_str(), "ok" | "error") {
        return Err(AgentError::invalid("invalid observation status"));
    }
    let content = required_string(object, "content")?;
    if content.len() > 16 * 1024 {
        return Err(AgentError::invalid("observation content exceeds limit"));
    }
    Ok(Some(AgentToolObservation {
        tool_call_id,
        name,
        status,
        content,
        truncated: object
            .get("truncated")
            .and_then(Value::as_bool)
            .ok_or_else(|| AgentError::invalid("invalid observation truncated"))?,
    }))
}

fn exact_object<'a>(
    value: &'a Value,
    keys: &[&str],
) -> AgentResult<&'a serde_json::Map<String, Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| AgentError::invalid("hosted invocation must be object"))?;
    if object.len() != keys.len() || !object.keys().all(|key| keys.contains(&key.as_str())) {
        return Err(AgentError::invalid(
            "unknown or missing hosted invocation field",
        ));
    }
    Ok(object)
}

fn required_string(object: &serde_json::Map<String, Value>, key: &str) -> AgentResult<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| AgentError::invalid(format!("invalid {key}")))
}

fn startup_handshake(agent: &str) -> Result<(), cortexfs_runtime_client::RuntimeClientError> {
    cortexfs_runtime_client::ping_from_environment(agent).map(|_| ())
}

/// Creates an owned child through the receipt-bound runtime capability.
pub fn create_child(
    child: &str,
    child_session: &str,
    input: &str,
) -> Result<cortexfs_runtime_client::CreateChildResult, cortexfs_runtime_client::RuntimeClientError>
{
    if !is_object_name(child)
        || !is_object_name(child_session)
        || input.contains('\0')
        || input.len() > MAX_CHILD_INPUT_BYTES
    {
        return Err(cortexfs_runtime_client::RuntimeClientError::InvalidEnvironment);
    }
    let request_id = cortexfs_runtime_client::fresh_request_id("agent-create")?;
    cortexfs_runtime_client::create_child_from_environment(&request_id, child, child_session, input)
}

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

fn env_text(name: &str) -> Option<String> {
    env::var_os(name).and_then(|value| value.into_string().ok())
}

fn collect_input_from_reader<I>(args: I, reader: impl Read) -> io::Result<String>
where
    I: IntoIterator<Item = OsString>,
{
    let input = args
        .into_iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    if !input.is_empty() {
        return Ok(input);
    }
    let limit = u64::try_from(MAX_CLI_STDIN_INPUT_BYTES.saturating_add(1))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut input = String::new();
    reader.take(limit).read_to_string(&mut input)?;
    if input.len() > MAX_CLI_STDIN_INPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "agent CLI stdin exceeds input limit",
        ));
    }
    Ok(input)
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

    #[test]
    #[ignore = "subprocess entrypoint for startup handshake test"]
    fn startup_handshake_subprocess() {
        assert_eq!(run_cli(&Echo, [OsString::from("hi")]), ExitCode::SUCCESS);
    }

    #[test]
    #[ignore = "subprocess entrypoint for child capability test"]
    fn create_child_subprocess() {
        assert!(create_child("worker-a", "child-a", "first handoff").is_ok());
        assert!(create_child("worker-b", "child-b", "second handoff").is_ok());
    }

    #[test]
    #[ignore = "subprocess entrypoint for hosted envelope test"]
    fn hosted_envelope_subprocess() {
        assert_eq!(
            run_cli(&Echo, [OsString::from(HOSTED_ENVELOPE_ARG)]),
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
        let output = std::process::Command::new(env::current_exe()?)
            .arg("--exact")
            .arg("tests::startup_handshake_subprocess")
            .arg("--ignored")
            .env("CTX_AGENT", "echo-agent")
            .env("CTX_SESSION", "live")
            .env("CTX_RUN_ID", "run-1")
            .env(&environment[0].0, &environment[0].1)
            .env(&environment[1].0, &environment[1].1)
            .output()?;
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
                        .send((request.child.clone(), request.input.clone()))
                        .map_err(|_error| {
                            cortexfs::runtime::control::RunCapabilityError::CannotCreate
                        })?;
                    Ok(cortexfs::runtime::control::CreateChildResult {
                        child: request.child,
                        child_session: request.child_session,
                        pid: 42,
                    })
                },
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
                ("worker-a".to_owned(), "first handoff".to_owned()),
                ("worker-b".to_owned(), "second handoff".to_owned())
            ]
        );
        shutdown.store(true, Ordering::Release);
        assert!(matches!(server.join(), Ok(Ok(()))));
        Ok(())
    }

    #[test]
    fn sdk_allows_standalone_and_rejects_partial_capability()
    -> Result<(), Box<dyn std::error::Error>> {
        let binary = env::current_exe()?;
        let standalone = std::process::Command::new(&binary)
            .arg("--exact")
            .arg("tests::startup_handshake_subprocess")
            .arg("--ignored")
            .env("CTX_AGENT", "echo-agent")
            .env("CTX_RUN_ID", "run-1")
            .env_remove("CTX_CONTROL_SOCKET")
            .env_remove("CTX_CONTROL_TOKEN")
            .status()?;
        assert!(standalone.success());
        let partial = std::process::Command::new(binary)
            .arg("--exact")
            .arg("tests::startup_handshake_subprocess")
            .arg("--ignored")
            .env("CTX_AGENT", "echo-agent")
            .env("CTX_RUN_ID", "run-1")
            .env("CTX_CONTROL_TOKEN", "partial")
            .env_remove("CTX_CONTROL_SOCKET")
            .output()?;
        assert!(!partial.status.success());
        assert!(
            !partial
                .stdout
                .windows(b"\"type\":\"start\"".len())
                .any(|bytes| bytes == b"\"type\":\"start\"")
        );
        Ok(())
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
    fn run_agent_emits_message_and_done() {
        let mut bytes = Vec::new();
        assert!(run_agent(&Echo, &AgentInvocation::new("r1", "hello"), &mut bytes).is_ok());
        let frames: Vec<Value> = String::from_utf8(bytes)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        assert_eq!(
            frames,
            vec![
                json!({ "type": "start", "run": "r1" }),
                json!({
                    "type": "message", "run": "r1", "role": "assistant",
                    "content": [{ "type": "text", "text": "hello" }]
                }),
                json!({ "type": "done", "run": "r1", "status": "ok" })
            ]
        );
    }

    #[test]
    fn agent_error_emits_ordered_frames_and_fails_status() {
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
        assert!(matches!(
            run_agent_status(&Fail, &AgentInvocation::new("r1", ""), &mut bytes, true),
            Ok(false)
        ));
        let types = String::from_utf8(bytes)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|frame| frame.get("type")?.as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(types, ["start", "error", "done"]);
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
        for event_type in ["start", "done", "error", "tool_call", "tool_result"] {
            let error = emitter.event(json!({ "type": event_type }));
            assert!(matches!(error, Err(ref error) if error.code() == "EINVAL"));
        }
        let frame: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        assert_eq!(frame, json!({ "type": "usage", "run": "current" }));
    }

    #[test]
    fn tool_yield_is_terminal_without_sdk_done() {
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
        assert!(run_agent(&Yield, &AgentInvocation::new("r1", ""), &mut bytes).is_ok());
        let frames = String::from_utf8(bytes).unwrap_or_default();
        let types = frames
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|frame| frame.get("type")?.as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(types, ["start", "tool_call"]);
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
    fn cli_input_accepts_argv_and_stdin_boundary() {
        assert_eq!(
            collect_input_from_reader(
                [OsString::from("hello"), OsString::from("world")],
                io::empty()
            )
            .unwrap_or_default(),
            "hello world"
        );
        let input = vec![b'x'; MAX_CLI_STDIN_INPUT_BYTES];
        assert_eq!(
            collect_input_from_reader(std::iter::empty::<OsString>(), Cursor::new(input))
                .unwrap_or_default()
                .len(),
            MAX_CLI_STDIN_INPUT_BYTES
        );
    }

    #[test]
    fn cli_input_rejects_oversized_stdin() {
        let input = vec![b'x'; MAX_CLI_STDIN_INPUT_BYTES.saturating_add(1)];
        let result = collect_input_from_reader(std::iter::empty::<OsString>(), Cursor::new(input));
        assert!(matches!(result, Err(ref error) if error.kind() == io::ErrorKind::InvalidData));
    }

    #[test]
    fn hosted_envelope_is_strict_and_typed() -> Result<(), Box<dyn std::error::Error>> {
        let input = br#"{"schema":"cortexfs.agent-invocation/v1","run":"r1","step":1,"input":"hello","history_messages":"[]","tool_context":"","observation":{"tool_call_id":"call-1","name":"example.echo","status":"ok","content":"a","truncated":false}}
"#;
        let envelope = parse_hosted_envelope(Cursor::new(input)).map_err(|error| {
            io::Error::other(format!(
                "agent SDK test failure: {}: {}",
                error.code(),
                error.message()
            ))
        })?;
        assert_eq!(envelope.step, 1);
        assert_eq!(
            envelope
                .observation
                .as_ref()
                .map(AgentToolObservation::content),
            Some("a")
        );
        let body = input
            .get(..input.len().saturating_sub(1))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing envelope body"))?;
        for invalid in [body, b"{}\n".as_slice(), b"{}\n{}\n".as_slice()] {
            assert!(parse_hosted_envelope(Cursor::new(invalid)).is_err());
        }
        assert!(parse_hosted_envelope(Cursor::new(vec![0xff, b'\n'])).is_err());
        let base: Value = serde_json::from_slice(body)?;
        for (field, value) in [
            ("schema", json!("wrong/v1")),
            ("step", json!(9)),
            ("observation", Value::Null),
            (
                "history_messages",
                json!("x".repeat(MAX_AGENT_CONTEXT_BYTES + 1)),
            ),
        ] {
            let mut invalid = base.clone();
            invalid
                .as_object_mut()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid envelope"))?
                .insert(field.to_owned(), value);
            let bytes = invalid.to_string() + "\n";
            assert!(
                parse_hosted_envelope(Cursor::new(bytes)).is_err(),
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
        assert!(parse_hosted_envelope(Cursor::new(bytes)).is_err());
        zero_with_observation
            .as_object_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid envelope"))?
            .insert("step".to_owned(), json!(0));
        let bytes = zero_with_observation.to_string() + "\n";
        assert!(parse_hosted_envelope(Cursor::new(bytes)).is_err());
        Ok(())
    }

    #[test]
    fn hosted_envelope_validates_run_and_step_without_lifecycle()
    -> Result<(), Box<dyn std::error::Error>> {
        let binary = env::current_exe()?;
        let envelope = |run: &str, step: u8| {
            serde_json::json!({
                "schema": AGENT_INVOCATION_SCHEMA, "run": run, "step": step,
                "input": "hello", "history_messages": "[]", "tool_context": "",
                "observation": Value::Null
            })
            .to_string()
                + "\n"
        };
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
                .env("CTX_AGENT_LAUNCH", HOSTED_ENVELOPE_ENV)
                .env("CTX_AGENT_STEP", env_step)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()?;
            let Some(mut stdin) = child.stdin.take() else {
                return Err(io::Error::other("child stdin unavailable").into());
            };
            stdin.write_all(envelope(run, 0).as_bytes())?;
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

    #[test]
    fn envelope_looking_stdin_remains_plain_without_host_markers() {
        let plain = "{\"schema\":\"cortexfs.agent-invocation/v1\"}\n";
        assert_eq!(
            collect_input_from_reader(std::iter::empty::<OsString>(), Cursor::new(plain))
                .unwrap_or_default(),
            plain
        );
    }
}
