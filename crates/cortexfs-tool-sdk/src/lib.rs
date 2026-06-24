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
        serde_json::from_str::<Value>(&self.input)
            .ok()?
            .get(field)?
            .as_str()
            .map(str::to_owned)
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
        self.frame(&json!({
            "type": "message",
            "run": self.run_id,
            "role": "tool",
            "content": [{ "type": "text", "text": text }]
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
pub trait Tool: Sync {
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
        run_tool(tool, invocation, writer).map_err(RegistryError::Io)
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
pub fn run_tool(
    tool: &dyn Tool,
    invocation: &ToolInvocation,
    writer: &mut dyn Write,
) -> io::Result<()> {
    let mut output = ToolEmitter::new(invocation.run_id().to_owned(), writer);
    output.start(tool.spec().name)?;
    match tool.call(invocation, &mut output) {
        Ok(()) => output.done("ok"),
        Err(error) => {
            output.error(&error)?;
            output.done("error")
        }
    }
}

/// Runs a tool as a normal CLI executable.
pub fn run_cli<I>(tool: &dyn Tool, args: I) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    match run_cli_inner(tool, args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_error) => ExitCode::from(1),
    }
}

fn run_cli_inner<I>(tool: &dyn Tool, args: I) -> io::Result<()>
where
    I: IntoIterator<Item = OsString>,
{
    let input = collect_input(args)?;
    let run_id = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let invocation = ToolInvocation::new(run_id, input);
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    run_tool(tool, &invocation, &mut stdout)
}

fn collect_input<I>(args: I) -> io::Result<String>
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
    io::stdin().read_to_string(&mut input)?;
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
        Registry, RegistryError, Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult,
        ToolSpec, collect_input, run_tool,
    };
    use std::ffi::OsString;
    use std::io::{self, Write};

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

    #[test]
    fn run_tool_emits_canonical_jsonl_frames() {
        let tool = EchoTool;
        let invocation = ToolInvocation::new("r-test", r#"{"text":"hello"}"#);
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());
        let output = String::from_utf8(output).unwrap_or_default();
        assert!(output.contains(r#""type":"start""#));
        assert!(output.contains(r#""tool":"test.echo""#));
        assert!(output.contains(r#""text":"hello""#));
        assert!(output.contains(r#""status":"ok""#));
    }

    #[test]
    fn run_tool_emits_error_frames() {
        let tool = EchoTool;
        let invocation = ToolInvocation::new("r-test", "");
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());
        let output = String::from_utf8(output).unwrap_or_default();
        assert!(output.contains(r#""code":"EINVAL""#));
        assert!(output.contains(r#""status":"error""#));
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
        assert_eq!(collect_input(args).unwrap_or_default(), "hello world");
    }

    #[test]
    fn registry_error_debug_is_available() {
        let error = RegistryError::Io(io::Error::other("boom"));
        let text = format!("{error:?}");
        assert!(text.contains("boom"));
    }
}
