//! Rust SDK for `CortexFS` tools.
//!
//! A tool written with this crate has one canonical implementation:
//! implement [`Tool`]. The same value can then be exposed as a normal `CLI`
//! binary with [`run_cli`] or called directly in-process through [`Registry`].

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::ffi::c_void;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::slice;

use libloading::Library;
use serde_json::{Value, json};

const TOOL_ABI_MAGIC_V1: u64 = 0x4354_5854_4f4f_4c31;

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

/// Borrowed byte string used by the stable dynamic tool ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolStr {
    /// UTF-8 byte pointer.
    pub ptr: *const u8,
    /// Byte length.
    pub len: usize,
}

// SAFETY: `ToolStr` is only used for immutable ABI strings. Producers must
// provide bytes that remain valid while the dynamic artifact is loaded.
unsafe impl Sync for ToolStr {}

impl ToolStr {
    /// Creates an ABI string from a static Rust string.
    #[must_use]
    pub const fn from_static(value: &'static str) -> Self {
        Self {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }

    fn as_str(self) -> io::Result<&'static str> {
        if self.ptr.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tool ABI string has null pointer",
            ));
        }
        // SAFETY: The dynamic tool ABI requires `ptr,len` to reference immutable
        // UTF-8 bytes that remain valid while the library is loaded. The loader
        // keeps the `Library` alive inside `DynamicTool`.
        let bytes = unsafe { slice::from_raw_parts(self.ptr, self.len) };
        std::str::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

/// Invocation passed over the dynamic tool ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolInvocationAbi {
    /// Run id.
    pub run: ToolStr,
    /// Raw input.
    pub input: ToolStr,
}

/// Writer callback passed over the dynamic tool ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ToolWriterAbi {
    /// Opaque writer context.
    pub ctx: *mut c_void,
    /// Writer callback.
    pub write: extern "C" fn(*mut c_void, *const u8, usize) -> i32,
}

/// Stable dynamic ABI descriptor exported by loadable tool artifacts.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ToolAbiV1 {
    magic: u64,
    version: u32,
    name: ToolStr,
    description: ToolStr,
    input_schema: ToolStr,
    call: extern "C" fn(ToolInvocationAbi, ToolWriterAbi) -> i32,
}

impl ToolAbiV1 {
    /// Creates a tool ABI descriptor.
    #[must_use]
    pub const fn new(
        name: &'static str,
        description: &'static str,
        input_schema: &'static str,
        call: extern "C" fn(ToolInvocationAbi, ToolWriterAbi) -> i32,
    ) -> Self {
        Self {
            magic: TOOL_ABI_MAGIC_V1,
            version: 1,
            name: ToolStr::from_static(name),
            description: ToolStr::from_static(description),
            input_schema: ToolStr::from_static(input_schema),
            call,
        }
    }

    fn validate(self) -> io::Result<Self> {
        if self.magic == TOOL_ABI_MAGIC_V1 && self.version == 1 {
            Ok(self)
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported CortexFS tool ABI",
            ))
        }
    }
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

/// Dynamically loaded single-file tool artifact.
#[derive(Debug)]
pub struct DynamicTool {
    _library: Library,
    abi: ToolAbiV1,
}

/// W-TinyLFU cache for dynamically loaded tool artifacts.
#[derive(Debug)]
pub struct DynamicToolCache {
    capacity: usize,
    window_capacity: usize,
    clock: u64,
    frequencies: BTreeMap<String, u64>,
    entries: BTreeMap<String, CachedDynamicTool>,
}

#[derive(Debug)]
struct CachedDynamicTool {
    path: String,
    last_used: u64,
    segment: CacheSegment,
    tool: DynamicTool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CacheSegment {
    Window,
    Main,
}

impl DynamicToolCache {
    /// Creates a cache with at least one slot.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let window_capacity = (capacity / 100).max(1).min(capacity);
        Self {
            capacity,
            window_capacity,
            clock: 0,
            frequencies: BTreeMap::new(),
            entries: BTreeMap::new(),
        }
    }

    /// Number of currently loaded dynamic tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when no tools are loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns true when a path is currently loaded.
    #[must_use]
    pub fn contains_path(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    /// Gets a loaded tool or loads it from disk, using W-TinyLFU admission
    /// when the cache is full.
    pub fn get_or_load(&mut self, path: impl AsRef<Path>) -> io::Result<&DynamicTool> {
        let path = path.as_ref().display().to_string();
        self.record_frequency(&path);
        if !self.entries.contains_key(&path) {
            let tool = DynamicTool::open(&path)?;
            self.entries.insert(
                path.clone(),
                CachedDynamicTool {
                    path: path.clone(),
                    last_used: 0,
                    segment: CacheSegment::Window,
                    tool,
                },
            );
            self.admit_window_candidate(&path);
        }
        self.clock = self.clock.saturating_add(1);
        let Some(entry) = self.entries.get_mut(&path) else {
            return Err(io::Error::other("dynamic tool cache insert failed"));
        };
        entry.last_used = self.clock;
        Ok(&entry.tool)
    }

    fn record_frequency(&mut self, path: &str) {
        let count = self.frequencies.entry(path.to_owned()).or_insert(0);
        *count = count.saturating_add(1);
    }

    fn admit_window_candidate(&mut self, current_path: &str) {
        while self.window_len() > self.window_capacity {
            let Some(candidate) = self.oldest_window_path() else {
                return;
            };
            if self.main_len() < self.main_capacity() {
                if let Some(entry) = self.entries.get_mut(&candidate) {
                    entry.segment = CacheSegment::Main;
                }
                continue;
            }
            let Some(victim) = self.main_victim_path() else {
                return;
            };
            if tiny_lfu_admits(
                self.frequency(&candidate),
                self.frequency(&victim),
                self.last_used(&candidate),
                self.last_used(&victim),
            ) {
                let _dropped = self.entries.remove(&victim);
                if let Some(entry) = self.entries.get_mut(&candidate) {
                    entry.segment = CacheSegment::Main;
                }
            } else if candidate != current_path {
                let _dropped = self.entries.remove(&candidate);
            } else {
                return;
            }
        }

        while self.entries.len() > self.capacity {
            let victim = self
                .main_victim_path()
                .or_else(|| self.oldest_window_path())
                .filter(|path| path != current_path);
            let Some(victim) = victim else {
                return;
            };
            let _dropped = self.entries.remove(&victim);
        }
    }

    fn window_len(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.segment == CacheSegment::Window)
            .count()
    }

    fn main_len(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.segment == CacheSegment::Main)
            .count()
    }

    fn main_capacity(&self) -> usize {
        self.capacity.saturating_sub(self.window_capacity).max(1)
    }

    fn oldest_window_path(&self) -> Option<String> {
        self.entries
            .values()
            .filter(|entry| entry.segment == CacheSegment::Window)
            .min_by_key(|entry| (entry.last_used, entry.path.clone()))
            .map(|entry| entry.path.clone())
    }

    fn main_victim_path(&self) -> Option<String> {
        wtinylfu_victim_path(
            self.entries
                .values()
                .filter(|entry| entry.segment == CacheSegment::Main)
                .map(|entry| {
                    (
                        entry.path.as_str(),
                        self.frequency(&entry.path),
                        entry.last_used,
                    )
                }),
        )
    }

    fn frequency(&self, path: &str) -> u64 {
        self.frequencies.get(path).copied().unwrap_or(0)
    }

    fn last_used(&self, path: &str) -> u64 {
        self.entries.get(path).map_or(0, |entry| entry.last_used)
    }
}

fn tiny_lfu_admits(
    candidate_frequency: u64,
    victim_frequency: u64,
    candidate_last_used: u64,
    victim_last_used: u64,
) -> bool {
    candidate_frequency > victim_frequency
        || (candidate_frequency == victim_frequency && candidate_last_used > victim_last_used)
}

fn wtinylfu_victim_path<'a>(
    entries: impl IntoIterator<Item = (&'a str, u64, u64)>,
) -> Option<String> {
    entries
        .into_iter()
        .min_by_key(|(path, hits, last_used)| (*hits, *last_used, (*path).to_owned()))
        .map(|(path, _hits, _last_used)| path.to_owned())
}

impl DynamicTool {
    /// Loads a tool artifact from disk.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        // SAFETY: Loading a dynamic library can run platform loader code. The
        // caller explicitly requested loading this tool artifact from disk.
        let library = unsafe { Library::new(path.as_ref()) }.map_err(io::Error::other)?;
        // SAFETY: `cortexfs_tool_abi_v1` is the stable symbol required by this
        // SDK. We copy the returned descriptor while keeping the library alive.
        let abi = unsafe {
            let symbol = library
                .get::<extern "C" fn() -> *const ToolAbiV1>(b"cortexfs_tool_abi_v1")
                .map_err(io::Error::other)?;
            let pointer = symbol();
            if pointer.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "tool ABI function returned null",
                ));
            }
            *pointer
        }
        .validate()?;
        Ok(Self {
            _library: library,
            abi,
        })
    }
}

impl Tool for DynamicTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.abi.name.as_str().unwrap_or("<invalid>"),
            description: self.abi.description.as_str().unwrap_or("<invalid>"),
            input_schema: self.abi.input_schema.as_str().unwrap_or("{}"),
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let mut writer = AbiWriter { output };
        let abi_invocation = ToolInvocationAbi {
            run: ToolStr {
                ptr: invocation.run_id().as_ptr(),
                len: invocation.run_id().len(),
            },
            input: ToolStr {
                ptr: invocation.input().as_ptr(),
                len: invocation.input().len(),
            },
        };
        let abi_writer = ToolWriterAbi {
            ctx: (&mut writer as *mut AbiWriter<'_, '_>).cast::<c_void>(),
            write: abi_write,
        };
        match (self.abi.call)(abi_invocation, abi_writer) {
            0 => Ok(()),
            _ => Err(ToolError::new("EIO", "dynamic tool failed")),
        }
    }
}

struct AbiWriter<'a, 'b> {
    output: &'a mut ToolEmitter<&'b mut dyn Write>,
}

extern "C" fn abi_write(ctx: *mut c_void, ptr: *const u8, len: usize) -> i32 {
    if ctx.is_null() || ptr.is_null() {
        return -1;
    }
    // SAFETY: The callee receives `ctx` from `DynamicTool::call`, where it was
    // created from a live mutable `AbiWriter`. The byte pointer is valid for the
    // duration of this callback by ABI contract.
    let writer = unsafe { &mut *ctx.cast::<AbiWriter<'_, '_>>() };
    // SAFETY: The ABI writer callback receives a byte slice valid for the call.
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    match writer.output.writer.write_all(bytes) {
        Ok(()) => 0,
        Err(_error) => -1,
    }
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

/// Exports a tool as a dynamic artifact and as a normal executable entry point.
#[macro_export]
macro_rules! cortexfs_tool_artifact {
    ($tool:expr, name: $name:expr, description: $description:expr, schema: $schema:expr $(,)?) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn cortexfs_tool_abi_v1() -> *const $crate::ToolAbiV1 {
            extern "C" fn call(
                invocation: $crate::ToolInvocationAbi,
                writer: $crate::ToolWriterAbi,
            ) -> i32 {
                struct AbiWrite($crate::ToolWriterAbi);

                impl std::io::Write for AbiWrite {
                    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                        let status = (self.0.write)(self.0.ctx, buffer.as_ptr(), buffer.len());
                        if status == 0 {
                            Ok(buffer.len())
                        } else {
                            Err(std::io::Error::other("tool ABI write failed"))
                        }
                    }

                    fn flush(&mut self) -> std::io::Result<()> {
                        Ok(())
                    }
                }

                fn read_abi(value: $crate::ToolStr) -> Result<&'static str, ()> {
                    if value.ptr.is_null() {
                        return Err(());
                    }
                    // SAFETY: Tool ABI strings are required to be valid for the
                    // duration of the call.
                    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
                    std::str::from_utf8(bytes).map_err(|_error| ())
                }

                let Ok(run) = read_abi(invocation.run) else {
                    return -1;
                };
                let Ok(input) = read_abi(invocation.input) else {
                    return -1;
                };
                let tool = $tool;
                let invocation = $crate::ToolInvocation::new(run, input);
                let mut writer = AbiWrite(writer);
                let mut output =
                    $crate::ToolEmitter::new(run, &mut writer as &mut dyn std::io::Write);
                match $crate::Tool::call(&tool, &invocation, &mut output) {
                    Ok(()) => 0,
                    Err(_error) => -1,
                }
            }

            static ABI: $crate::ToolAbiV1 =
                $crate::ToolAbiV1::new($name, $description, $schema, call);
            &ABI
        }

        fn main() -> std::process::ExitCode {
            $crate::run_cli(&$tool, std::env::args_os().skip(1))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{
        Registry, RegistryError, Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult,
        ToolSpec, collect_input, run_tool, tiny_lfu_admits, wtinylfu_victim_path,
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

    #[test]
    fn wtinylfu_victim_prefers_lowest_frequency_then_oldest_use() {
        assert_eq!(
            wtinylfu_victim_path([("/tool/a", 4, 10), ("/tool/b", 1, 20), ("/tool/c", 1, 5),]),
            Some("/tool/c".to_owned())
        );
        assert_eq!(wtinylfu_victim_path([]), None);
    }

    #[test]
    fn tiny_lfu_admission_keeps_frequent_main_entries() {
        assert!(tiny_lfu_admits(5, 2, 1, 10));
        assert!(!tiny_lfu_admits(1, 3, 10, 1));
        assert!(tiny_lfu_admits(2, 2, 20, 10));
        assert!(!tiny_lfu_admits(2, 2, 10, 20));
    }
}
