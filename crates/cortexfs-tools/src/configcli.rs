use crate::config::apply_request;
use cortexfs_tool_sdk::{Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

#[derive(Debug)]
pub struct TshConfigTool;

impl Tool for TshConfigTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "tsh.config",
            description: "Read or update persistent tsh runtime configuration.",
            input_schema: crate::configschema::TSH_CONFIG_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let (path, config) = apply_request(&crate::ctx_root_from_env(), invocation.input())?;
        output
            .message(&format!(
                "{}\n{}",
                path.display(),
                crate::format_tsh_runtime_config(config)
            ))
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

pub(crate) fn run_tsh_config_cli(
    root: &Path,
    args: &[OsString],
    writer: &mut dyn Write,
) -> io::Result<ExitCode> {
    let input = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let (path, config) =
        apply_request(root, &input).map_err(|error| crate::tool_error_to_io(&error))?;
    writeln!(writer, "{}", path.display())?;
    writer.write_all(crate::format_tsh_runtime_config(config).as_bytes())?;
    Ok(ExitCode::SUCCESS)
}
