use cortexfs_tool_sdk::{
    Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec, run_tool,
};
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Debug)]
pub struct FsReadTool;

#[derive(Debug)]
pub struct FsWriteTool;

#[derive(Debug)]
pub struct ShellExecTool;

#[derive(Debug)]
pub struct TshConfigTool;

impl Tool for FsReadTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.read",
            description: "Read a UTF-8 text file from the visible filesystem.",
            input_schema: FS_READ_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation
            .string_field("path")
            .unwrap_or_else(|| invocation.input().trim().to_owned());
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        match fs::read_to_string(&path) {
            Ok(content) => output
                .message(&content)
                .map_err(|error| ToolError::new("EIO", error.to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(ToolError::not_found("file not found"))
            }
            Err(_error) => Err(ToolError::denied("read failed")),
        }
    }
}

impl Tool for FsWriteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.write",
            description: "Write UTF-8 text to a path in the visible filesystem.",
            input_schema: FS_WRITE_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation.string_field("path").unwrap_or_default();
        let content = invocation.string_field("content").unwrap_or_default();
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        fs::write(path, content).map_err(|_error| ToolError::denied("write failed"))?;
        output
            .message("written")
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

impl Tool for ShellExecTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell.exec",
            description: "Run one shell command in the tool process environment.",
            input_schema: SHELL_EXEC_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let command = invocation
            .string_field("cmd")
            .unwrap_or_else(|| invocation.input().trim().to_owned());
        if command.is_empty() {
            return Err(ToolError::invalid("missing cmd"));
        }
        let command_output = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .output()
            .map_err(|error| ToolError::new("EIO", format!("cannot run shell command: {error}")))?;
        let mut text = String::from_utf8_lossy(&command_output.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&command_output.stderr));
        output
            .message(&text)
            .map_err(|error| ToolError::new("EIO", error.to_string()))?;
        if command_output.status.success() {
            Ok(())
        } else {
            Err(ToolError::new("EIO", "command failed"))
        }
    }
}

impl Tool for TshConfigTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "tsh.config",
            description: "Read or update persistent tsh runtime configuration.",
            input_schema: TSH_CONFIG_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let input = invocation.input().trim();
        let request = if input.is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str::<Value>(input)
                .map_err(|_error| ToolError::invalid("invalid json input"))?
        };
        let Some(object) = request.as_object() else {
            return Err(ToolError::invalid("input must be a json object"));
        };
        let path = requested_tsh_config_path(object)?;
        let mut config = read_tsh_runtime_config(&path)?;
        let changed = object.contains_key("max_loaded_tools")
            || object.contains_key("cache_capacity")
            || object.contains_key("window_percent");
        if let Some(value) = object.get("max_loaded_tools") {
            config.max_loaded_tools = positive_usize(value, "max_loaded_tools")?;
        }
        if let Some(value) = object.get("cache_capacity") {
            config.cache_capacity = positive_usize(value, "cache_capacity")?;
        }
        if let Some(value) = object.get("window_percent") {
            let window_percent = positive_usize(value, "window_percent")?;
            if !(1..=100).contains(&window_percent) {
                return Err(ToolError::invalid("window_percent must be 1..100"));
            }
            config.window_percent = window_percent;
        }
        if changed {
            write_tsh_runtime_config(&path, config)?;
        }
        output
            .message(&format!(
                "{}\n{}",
                path.display(),
                format_tsh_runtime_config(config)
            ))
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

#[must_use]
pub fn core_tool_specs() -> Vec<ToolSpec> {
    vec![
        FsReadTool.spec(),
        FsWriteTool.spec(),
        ShellExecTool.spec(),
        TshConfigTool.spec(),
    ]
}

pub fn run_core_tool(
    name: &str,
    invocation: &ToolInvocation,
    writer: &mut dyn Write,
) -> Result<bool, io::Error> {
    match name {
        "fs.read" => run_tool(&FsReadTool, invocation, writer).map(|()| true),
        "fs.write" => run_tool(&FsWriteTool, invocation, writer).map(|()| true),
        "shell.exec" => run_tool(&ShellExecTool, invocation, writer).map(|()| true),
        "tsh.config" => run_tool(&TshConfigTool, invocation, writer).map(|()| true),
        _ => Ok(false),
    }
}

pub fn run_core_tool_cli(
    name: &str,
    args: &[OsString],
    writer: &mut dyn Write,
) -> Result<Option<ExitCode>, io::Error> {
    match name {
        "fs.read" => run_fs_read_cli(args, writer).map(Some),
        "fs.write" => run_fs_write_cli(args, writer).map(Some),
        "shell.exec" => run_shell_exec_cli(args, writer).map(Some),
        "tsh.config" => run_tsh_config_cli(args, writer).map(Some),
        _ => Ok(None),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TshRuntimeConfig {
    max_loaded_tools: usize,
    cache_capacity: usize,
    window_percent: usize,
}

impl Default for TshRuntimeConfig {
    fn default() -> Self {
        Self {
            max_loaded_tools: 64,
            cache_capacity: 32,
            window_percent: 1,
        }
    }
}

fn default_tsh_config_path() -> PathBuf {
    std::env::var_os("CTX_ROOT").map_or_else(
        || PathBuf::from("/ctx/tool/tsh.d/config"),
        |root| PathBuf::from(root).join("tool/tsh.d/config"),
    )
}

fn requested_tsh_config_path(object: &Map<String, Value>) -> ToolResult<PathBuf> {
    let default_path = default_tsh_config_path();
    let Some(value) = object.get("path") else {
        return Ok(default_path);
    };
    let Some(path) = value.as_str() else {
        return Err(ToolError::invalid("path must be a string"));
    };
    let requested_path = PathBuf::from(path);
    if requested_path == default_path {
        Ok(default_path)
    } else {
        Err(ToolError::denied(
            "tsh.config path is restricted to CTX_ROOT/tool/tsh.d/config",
        ))
    }
}

fn read_tsh_runtime_config(path: &Path) -> ToolResult<TshRuntimeConfig> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TshRuntimeConfig::default());
        }
        Err(error) => return Err(ToolError::denied(format!("cannot read config: {error}"))),
    };
    parse_tsh_runtime_config(&content)
}

fn parse_tsh_runtime_config(content: &str) -> ToolResult<TshRuntimeConfig> {
    let mut config = TshRuntimeConfig::default();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(ToolError::invalid(format!(
                "line {} must be key=value",
                index.saturating_add(1)
            )));
        };
        let value = value.parse::<usize>().map_err(|_error| {
            ToolError::invalid(format!(
                "line {} value must be a positive integer",
                index.saturating_add(1)
            ))
        })?;
        match key {
            "max_loaded_tools" if value > 0 => config.max_loaded_tools = value,
            "cache_capacity" if value > 0 => config.cache_capacity = value,
            "window_percent" if (1..=100).contains(&value) => config.window_percent = value,
            "max_loaded_tools" | "cache_capacity" => {
                return Err(ToolError::invalid(format!(
                    "line {} value must be greater than zero",
                    index.saturating_add(1)
                )));
            }
            "window_percent" => {
                return Err(ToolError::invalid(format!(
                    "line {} window_percent must be 1..100",
                    index.saturating_add(1)
                )));
            }
            _ => {
                return Err(ToolError::invalid(format!(
                    "line {} has unknown key {key}",
                    index.saturating_add(1)
                )));
            }
        }
    }
    Ok(config)
}

fn write_tsh_runtime_config(path: &Path, config: TshRuntimeConfig) -> ToolResult<()> {
    let Some(parent) = path.parent() else {
        return Err(ToolError::invalid(
            "config path must have a parent directory",
        ));
    };
    fs::create_dir_all(parent)
        .map_err(|error| ToolError::denied(format!("cannot create config directory: {error}")))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ToolError::invalid("config path must end with a valid UTF-8 file name"))?;
    let content = format_tsh_runtime_config(config);
    for attempt in 0..16 {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let tmp = parent.join(format!(
            ".{file_name}.tmp-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(content.as_bytes()) {
                    let _ignored = fs::remove_file(&tmp);
                    return Err(ToolError::denied(format!("cannot write config: {error}")));
                }
                if let Err(error) = file.sync_all() {
                    let _ignored = fs::remove_file(&tmp);
                    return Err(ToolError::denied(format!("cannot sync config: {error}")));
                }
                drop(file);
                return fs::rename(&tmp, path)
                    .map_err(|error| ToolError::denied(format!("cannot install config: {error}")));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(ToolError::denied(format!(
                    "cannot create config temp file: {error}"
                )));
            }
        }
    }
    Err(ToolError::denied("cannot create unique config temp file"))
}

fn format_tsh_runtime_config(config: TshRuntimeConfig) -> String {
    format!(
        "max_loaded_tools={}\ncache_capacity={}\nwindow_percent={}\n",
        config.max_loaded_tools, config.cache_capacity, config.window_percent
    )
}

fn positive_usize(value: &Value, field: &str) -> ToolResult<usize> {
    let Some(value) = value.as_u64() else {
        return Err(ToolError::invalid(format!(
            "{field} must be a positive integer"
        )));
    };
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| ToolError::invalid(format!("{field} must be a positive integer")))
}

fn run_fs_read_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let Some(path) = args.first() else {
        writeln!(io::stderr(), "fs.read: missing path")?;
        return Ok(ExitCode::from(2));
    };
    let content = fs::read_to_string(PathBuf::from(path))?;
    writer.write_all(content.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

fn run_fs_write_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let Some(path) = args.first() else {
        writeln!(io::stderr(), "fs.write: missing path")?;
        return Ok(ExitCode::from(2));
    };
    let content = if args.len() > 1 {
        args.iter()
            .skip(1)
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        let mut content = String::new();
        io::stdin().read_to_string(&mut content)?;
        content
    };
    fs::write(PathBuf::from(path), content)?;
    writeln!(writer, "written")?;
    Ok(ExitCode::SUCCESS)
}

fn run_shell_exec_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let command = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    if command.is_empty() {
        writeln!(io::stderr(), "shell.exec: missing command")?;
        return Ok(ExitCode::from(2));
    }
    let output = Command::new("sh").arg("-c").arg(command).output()?;
    writer.write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    Ok(exit_code_from_status(output.status))
}

fn run_tsh_config_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let input = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let request = if input.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str::<Value>(&input).map_err(io::Error::other)?
    };
    let object = request.as_object().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "input must be a json object")
    })?;
    let path = requested_tsh_config_path(object).map_err(|error| tool_error_to_io(&error))?;
    let mut config = read_tsh_runtime_config(&path).map_err(|error| tool_error_to_io(&error))?;
    let changed = object.contains_key("max_loaded_tools")
        || object.contains_key("cache_capacity")
        || object.contains_key("window_percent");
    if let Some(value) = object.get("max_loaded_tools") {
        config.max_loaded_tools =
            positive_usize(value, "max_loaded_tools").map_err(|error| tool_error_to_io(&error))?;
    }
    if let Some(value) = object.get("cache_capacity") {
        config.cache_capacity =
            positive_usize(value, "cache_capacity").map_err(|error| tool_error_to_io(&error))?;
    }
    if let Some(value) = object.get("window_percent") {
        let window_percent =
            positive_usize(value, "window_percent").map_err(|error| tool_error_to_io(&error))?;
        if !(1..=100).contains(&window_percent) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "window_percent must be 1..100",
            ));
        }
        config.window_percent = window_percent;
    }
    if changed {
        write_tsh_runtime_config(&path, config).map_err(|error| tool_error_to_io(&error))?;
    }
    writeln!(writer, "{}", path.display())?;
    writer.write_all(format_tsh_runtime_config(config).as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

fn exit_code_from_status(status: std::process::ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(1), ExitCode::from)
}

fn tool_error_to_io(error: &ToolError) -> io::Error {
    io::Error::other(format!("{}: {}", error.code(), error.message()))
}

const FS_READ_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.read input",
  "description": "Read one UTF-8 text file visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to a UTF-8 text file visible to the tool process."
    }
  }
}"#;

const FS_WRITE_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.write input",
  "description": "Write UTF-8 text to one path visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path", "content"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to write."
    },
    "content": {
      "type": "string",
      "description": "UTF-8 content to write."
    }
  }
}"#;

const SHELL_EXEC_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "shell.exec input",
  "description": "Run one shell command in the tool process environment.",
  "type": "object",
  "additionalProperties": false,
  "required": ["cmd"],
  "properties": {
    "cmd": {
      "type": "string",
      "description": "Command line passed to sh -c."
    }
  }
}"#;

const TSH_CONFIG_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tsh.config input",
  "description": "Read or update tsh.d/config. Omit all fields to show the current config.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "path": {
      "type": "string",
      "description": "Optional config path. If supplied, it must equal CTX_ROOT/tool/tsh.d/config or /ctx/tool/tsh.d/config."
    },
    "max_loaded_tools": {
      "type": "integer",
      "minimum": 1,
      "description": "Maximum unpinned tool metadata entries kept in the tsh context."
    },
    "cache_capacity": {
      "type": "integer",
      "minimum": 1,
      "description": "Maximum unpinned dynamic tool artifacts kept resident by W-TinyLFU."
    },
    "window_percent": {
      "type": "integer",
      "minimum": 1,
      "maximum": 100,
      "description": "Percentage of the dynamic cache used as the W-TinyLFU admission window."
    }
  }
}"#;

#[cfg(test)]
mod tests {
    use super::{FsReadTool, FsWriteTool, ShellExecTool, TshConfigTool, run_core_tool_cli};
    use cortexfs_tool_sdk::{ToolInvocation, run_tool};
    use std::ffi::OsString;
    use std::fs;

    #[test]
    fn fs_read_tool_emits_file_content() {
        let path = std::env::temp_dir().join(format!("cortexfs-fs-read-{}", std::process::id()));
        assert!(fs::write(&path, "visible").is_ok());
        let tool = FsReadTool;
        let invocation = ToolInvocation::new("r1", format!(r#"{{"path":"{}"}}"#, path.display()));
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());
        let text = String::from_utf8(output).unwrap_or_default();
        assert!(text.contains(r#""tool":"fs.read""#));
        assert!(text.contains(r#""text":"visible""#));
        let _ignored = fs::remove_file(path);
    }

    #[test]
    fn fs_write_tool_writes_file_content() {
        let path = std::env::temp_dir().join(format!("cortexfs-fs-write-{}", std::process::id()));
        let tool = FsWriteTool;
        let invocation = ToolInvocation::new(
            "r1",
            format!(r#"{{"path":"{}","content":"stored"}}"#, path.display()),
        );
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());
        assert_eq!(fs::read_to_string(&path).unwrap_or_default(), "stored");
        let _ignored = fs::remove_file(path);
    }

    #[test]
    fn shell_exec_tool_returns_stdout() {
        let tool = ShellExecTool;
        let invocation = ToolInvocation::new("r1", r#"{"cmd":"printf shell-ok"}"#);
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());
        let text = String::from_utf8(output).unwrap_or_default();
        assert!(text.contains(r#""tool":"shell.exec""#));
        assert!(text.contains("shell-ok"));
    }

    #[test]
    fn tsh_config_writer_updates_runtime_config_file() {
        let dir = std::env::temp_dir().join(format!("cortexfs-tsh-config-{}", std::process::id()));
        let path = dir.join("tool/tsh.d/config");
        let config = super::TshRuntimeConfig {
            max_loaded_tools: 12,
            cache_capacity: 6,
            window_percent: 10,
        };
        assert!(super::write_tsh_runtime_config(&path, config).is_ok());
        let config = fs::read_to_string(&path).unwrap_or_default();
        assert!(config.contains("max_loaded_tools=12\n"));
        assert!(config.contains("cache_capacity=6\n"));
        assert!(config.contains("window_percent=10\n"));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn tsh_config_tool_rejects_non_default_path() {
        let tool = TshConfigTool;
        let invocation = ToolInvocation::new(
            "r1",
            r#"{"path":"/tmp/cortexfs-outside-config","max_loaded_tools":12}"#,
        );
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());
        let text = String::from_utf8(output).unwrap_or_default();
        assert!(text.contains(r#""code":"EACCES""#));
        assert!(text.contains(r#""status":"error""#));
    }

    #[test]
    fn fs_read_cli_outputs_plain_text() {
        let path =
            std::env::temp_dir().join(format!("cortexfs-fs-read-cli-{}", std::process::id()));
        assert!(fs::write(&path, "plain").is_ok());
        let mut output = Vec::new();
        let result = run_core_tool_cli("fs.read", &[OsString::from(&path)], &mut output);
        assert!(matches!(result, Ok(Some(code)) if code == std::process::ExitCode::SUCCESS));
        assert_eq!(String::from_utf8(output).unwrap_or_default(), "plain");
        let _ignored = fs::remove_file(path);
    }
}
