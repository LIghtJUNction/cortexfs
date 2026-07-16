//! Rust SDK for `CortexFS` tools.
//!
//! A tool written with this crate has one canonical implementation:
//! implement [`Tool`]. The same value can then be exposed as a normal `CLI`
//! binary with [`run_cli`] or called directly in-process through [`Registry`].

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use serde_json::{Value, json};

const MAX_CLI_STDIN_INPUT_BYTES: usize = 1024 * 1024;

/// Static metadata exported by a `CortexFS` tool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolSpec {
    /// Stable tool name, such as `fs.read` or `project.echo`.
    pub name: &'static str,
    /// Short human-readable description.
    pub description: &'static str,
    /// JSON Schema text for the tool input.
    pub input_schema: &'static str,
}

/// One tool invocation.
#[derive(Debug, Eq, PartialEq)]
pub struct ToolInvocation {
    run_id: String,
    input: String,
}

impl ToolInvocation {
    /// Creates an invocation from a run id and raw input text.
    #[must_use]
    pub fn new(run_id: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            input: input.into(),
        }
    }

    /// Stable run id used in emitted JSONL frames.
    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Raw input as provided by CLI args, stdin, or an in-process caller.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Parses input as JSON.
    pub fn json(&self) -> Result<Value, ToolError> {
        serde_json::from_str(&self.input).map_err(|_error| ToolError::invalid("invalid json input"))
    }

    /// Reads a string field from JSON input. Returns `None` for invalid JSON,
    /// missing fields, and non-string values.
    #[must_use]
    pub fn string_field(&self, field: &str) -> Option<String> {
        self.value_field(field)?.as_str().map(str::to_owned)
    }

    /// Reads an optional JSON field from input.
    #[must_use]
    pub fn value_field(&self, field: &str) -> Option<Value> {
        self.json().ok()?.get(field).cloned()
    }

    /// Reads a required string field from JSON input.
    pub fn required_string_field(&self, field: &str) -> ToolResult<String> {
        self.json()?
            .get(field)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .ok_or_else(|| ToolError::invalid(format!("missing string field: {field}")))
    }

    /// Reads a required JSON field from input.
    pub fn required_value_field(&self, field: &str) -> ToolResult<Value> {
        self.json()?
            .get(field)
            .cloned()
            .ok_or_else(|| ToolError::invalid(format!("missing field: {field}")))
    }
}

/// Error returned by a tool implementation.
#[derive(Debug, Eq, PartialEq)]
pub struct ToolError {
    code: &'static str,
    message: String,
}

impl ToolError {
    /// Creates a tool error with a stable errno-like code.
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Invalid input error.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("EINVAL", message)
    }

    /// Permission error.
    #[must_use]
    pub fn denied(message: impl Into<String>) -> Self {
        Self::new("EACCES", message)
    }

    /// Missing resource error.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("ENOENT", message)
    }

    /// Stable error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Human-readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Result type used by tool implementations.
pub type ToolResult<T> = Result<T, ToolError>;

/// JSONL output stream for a tool invocation.
#[derive(Debug)]
pub struct ToolEmitter<W> {
    run_id: String,
    writer: W,
}

impl<W: Write> ToolEmitter<W> {
    /// Creates an emitter for a run id.
    pub fn new(run_id: impl Into<String>, writer: W) -> Self {
        Self {
            run_id: run_id.into(),
            writer,
        }
    }

    /// Emits a tool message frame with text content.
    pub fn message(&mut self, text: &str) -> io::Result<()> {
        self.content(&[json!({ "type": "text", "text": text })])
    }

    /// Emits a tool message frame with borrowed structured content blocks.
    pub fn content(&mut self, content: &[Value]) -> io::Result<()> {
        self.frame(&json!({
            "type": "message",
            "run": self.run_id,
            "role": "tool",
            "content": content
        }))
    }

    /// Emits a tool message frame with JSON content encoded as text.
    pub fn json_message(&mut self, value: &Value) -> io::Result<()> {
        self.message(&value.to_string())
    }

    fn start(&mut self, tool: &str) -> io::Result<()> {
        self.frame(&json!({ "type": "start", "run": self.run_id, "tool": tool }))
    }

    fn done(&mut self, status: &str) -> io::Result<()> {
        self.frame(&json!({ "type": "done", "run": self.run_id, "status": status }))
    }

    fn error(&mut self, error: &ToolError) -> io::Result<()> {
        self.frame(&json!({
            "type": "error",
            "run": self.run_id,
            "code": error.code(),
            "message": error.message()
        }))
    }

    fn frame(&mut self, value: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, value)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

/// `CortexFS` tool implementation.
pub trait Tool {
    /// Static tool metadata.
    fn spec(&self) -> ToolSpec;

    /// Executes the tool.
    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()>;
}

/// In-process tool registry.
#[derive(Clone, Copy)]
pub struct Registry<'a> {
    tools: &'a [&'a dyn Tool],
}

impl fmt::Debug for Registry<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("tool_count", &self.tools.len())
            .finish()
    }
}

impl<'a> Registry<'a> {
    /// Creates a registry over statically-linked tools.
    #[must_use]
    pub const fn new(tools: &'a [&'a dyn Tool]) -> Self {
        Self { tools }
    }

    /// Finds a tool by stable name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&'a dyn Tool> {
        self.tools
            .iter()
            .copied()
            .find(|tool| tool.spec().name == name)
    }

    /// Lists all registered tool specs.
    #[must_use]
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|tool| tool.spec()).collect()
    }

    /// Calls a registered tool without spawning a process.
    pub fn call(
        &self,
        name: &str,
        invocation: &ToolInvocation,
        writer: &mut dyn Write,
    ) -> Result<(), RegistryError> {
        let Some(tool) = self.find(name) else {
            return Err(RegistryError::NotFound);
        };
        run_tool(tool, invocation, writer)
            .map(|_code| ())
            .map_err(RegistryError::Io)
    }
}

/// Registry call failure.
#[derive(Debug)]
pub enum RegistryError {
    /// No registered tool has the requested name.
    NotFound,
    /// Writing JSONL output failed.
    Io(io::Error),
}

/// Runs a tool invocation and emits `CortexFS` JSONL frames.
///
/// Returns success after a canonical `done(ok)` frame, status `1` after a
/// [`ToolError`] and canonical error frames, or an I/O error when frames cannot
/// be written.
pub fn run_tool(
    tool: &dyn Tool,
    invocation: &ToolInvocation,
    writer: &mut dyn Write,
) -> io::Result<ExitCode> {
    let mut output = ToolEmitter::new(invocation.run_id().to_owned(), writer);
    output.start(tool.spec().name)?;
    match tool.call(invocation, &mut output) {
        Ok(()) => {
            output.done("ok")?;
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            output.error(&error)?;
            output.done("error")?;
            Ok(ExitCode::from(1))
        }
    }
}

/// Parses JSONL output into ordered frames.
pub fn parse_jsonl_frames(content: &str) -> serde_json::Result<Vec<Value>> {
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<Value>)
        .collect()
}

/// Runs a tool as a normal CLI executable.
pub fn run_cli<I>(tool: &dyn Tool, args: I) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    match run_cli_inner(tool, args) {
        Ok(code) => code,
        Err(_error) => ExitCode::from(1),
    }
}

fn run_cli_inner<I>(tool: &dyn Tool, args: I) -> io::Result<ExitCode>
where
    I: IntoIterator<Item = OsString>,
{
    let stdin = io::stdin();
    let input = collect_input_from_reader(args, stdin.lock())?;
    let run_id = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let invocation = ToolInvocation::new(run_id, input);
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    run_tool(tool, &invocation, &mut stdout)
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
    let mut input = String::new();
    let limit = u64::try_from(MAX_CLI_STDIN_INPUT_BYTES.saturating_add(1))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut reader = reader.take(limit);
    reader.read_to_string(&mut input)?;
    if input.len() > MAX_CLI_STDIN_INPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "tool CLI stdin exceeds input limit",
        ));
    }
    Ok(input)
}

/// Defines a CLI entry point for a tool value.
#[macro_export]
macro_rules! cortexfs_tool_main {
    ($tool:expr) => {
        fn main() -> std::process::ExitCode {
            $crate::run_cli(&$tool, std::env::args_os().skip(1))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_CLI_STDIN_INPUT_BYTES, Registry, RegistryError, Tool, ToolEmitter, ToolError,
        ToolInvocation, ToolResult, ToolSpec, Value, collect_input_from_reader, parse_jsonl_frames,
        run_tool,
    };
    use std::ffi::OsString;
    use std::io::{self, Cursor, Write};
    use std::process::ExitCode;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(Debug)]
    struct EchoTool;

    impl Tool for EchoTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "test.echo",
                description: "echo test input",
                input_schema: r#"{"type":"object"}"#,
            }
        }

        fn call(
            &self,
            invocation: &ToolInvocation,
            output: &mut ToolEmitter<&mut dyn Write>,
        ) -> ToolResult<()> {
            let text = invocation
                .string_field("text")
                .unwrap_or_else(|| invocation.input().trim().to_owned());
            if text.is_empty() {
                return Err(ToolError::invalid("missing text"));
            }
            output
                .message(&text)
                .map_err(|error| ToolError::new("EIO", error.to_string()))
        }
    }

    #[derive(Debug)]
    struct FailWriter;

    impl Write for FailWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("write failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn run_tool_emits_canonical_jsonl_frames() -> TestResult {
        let tool = EchoTool;
        let invocation = ToolInvocation::new("r-test", r#"{"text":"hello"}"#);
        let mut output = Vec::new();
        assert_eq!(
            run_tool(&tool, &invocation, &mut output)?,
            ExitCode::SUCCESS
        );
        let output = String::from_utf8(output)?;
        assert_eq!(
            parse_jsonl_frames(&output)?,
            vec![
                serde_json::json!({"type":"start","run":"r-test","tool":"test.echo"}),
                serde_json::json!({
                    "type": "message",
                    "run": "r-test",
                    "role": "tool",
                    "content": [{"type":"text","text":"hello"}]
                }),
                serde_json::json!({"type":"done","run":"r-test","status":"ok"}),
            ]
        );
        Ok(())
    }

    #[test]
    fn tool_emitter_content_emits_blocks_directly() -> TestResult {
        let content = [
            serde_json::json!({"type":"text","text":"hello"}),
            serde_json::json!({"type":"resource_link","uri":"file:///result"}),
        ];
        let mut output = Vec::new();
        let mut emitter = ToolEmitter::new("r-test", &mut output);

        emitter.content(&content)?;

        let output = String::from_utf8(output)?;
        assert_eq!(
            parse_jsonl_frames(&output)?,
            vec![serde_json::json!({
                "type": "message",
                "run": "r-test",
                "role": "tool",
                "content": content
            })]
        );
        Ok(())
    }

    #[test]
    fn run_tool_emits_error_frames() -> TestResult {
        let tool = EchoTool;
        let invocation = ToolInvocation::new("r-test", "");
        let mut output = Vec::new();
        assert_eq!(
            run_tool(&tool, &invocation, &mut output)?,
            ExitCode::from(1)
        );
        let output = String::from_utf8(output)?;
        assert_eq!(
            parse_jsonl_frames(&output)?,
            vec![
                serde_json::json!({"type":"start","run":"r-test","tool":"test.echo"}),
                serde_json::json!({
                    "type":"error",
                    "run":"r-test",
                    "code":"EINVAL",
                    "message":"missing text"
                }),
                serde_json::json!({"type":"done","run":"r-test","status":"error"}),
            ]
        );
        Ok(())
    }

    #[test]
    fn run_tool_propagates_writer_error() {
        let tool = EchoTool;
        let invocation = ToolInvocation::new("r-test", r#"{"text":"hello"}"#);
        let mut output = FailWriter;

        assert!(matches!(
            run_tool(&tool, &invocation, &mut output),
            Err(ref error) if error.to_string().contains("write failed")
        ));
    }

    #[test]
    fn tool_invocation_required_field_accessors() {
        let invocation = ToolInvocation::new("r-test", r#"{"text":"hello","count":2}"#);
        assert_eq!(
            invocation.required_string_field("text"),
            Ok("hello".to_owned())
        );
        assert!(matches!(
            invocation.required_string_field("missing"),
            Err(ref error) if error.code() == "EINVAL"
        ));
        assert_eq!(
            invocation.value_field("count"),
            Some(Value::Number(2u64.into()))
        );
        assert_eq!(
            invocation.required_value_field("count"),
            Ok(Value::Number(2u64.into()))
        );
        assert_eq!(invocation.value_field("missing"), None);

        let invalid = ToolInvocation::new("r-test", "{");
        assert!(matches!(
            invalid.required_string_field("text"),
            Err(ref error) if error.code() == "EINVAL" && error.message() == "invalid json input"
        ));
    }

    #[test]
    fn registry_calls_tool_without_process_spawn() {
        let tool = EchoTool;
        let tools: [&dyn Tool; 1] = [&tool];
        let registry = Registry::new(&tools);
        let invocation = ToolInvocation::new("r-test", "hello");
        let mut output = Vec::new();
        assert!(registry.call("test.echo", &invocation, &mut output).is_ok());
        assert!(matches!(
            registry.call("missing", &invocation, &mut output),
            Err(RegistryError::NotFound)
        ));
    }

    #[test]
    fn cli_input_collector_joins_args_as_input() {
        let args = [OsString::from("hello"), OsString::from("world")];
        assert_eq!(
            collect_input_from_reader(args, io::empty()).unwrap_or_default(),
            "hello world"
        );
    }

    #[test]
    fn cli_input_collector_accepts_stdin_at_limit() {
        let input = vec![b'x'; MAX_CLI_STDIN_INPUT_BYTES];

        let collected =
            collect_input_from_reader(std::iter::empty::<OsString>(), Cursor::new(input))
                .unwrap_or_default();

        assert_eq!(collected.len(), MAX_CLI_STDIN_INPUT_BYTES);
    }

    #[test]
    fn cli_input_collector_rejects_oversized_stdin() {
        let input = vec![b'x'; MAX_CLI_STDIN_INPUT_BYTES.saturating_add(1)];

        let collected =
            collect_input_from_reader(std::iter::empty::<OsString>(), Cursor::new(input));

        assert!(matches!(
            collected,
            Err(ref error) if error.kind() == io::ErrorKind::InvalidData
        ));
    }

    #[test]
    fn registry_error_debug_is_available() {
        let error = RegistryError::Io(io::Error::other("boom"));
        let text = format!("{error:?}");
        assert!(text.contains("boom"));
    }
}
