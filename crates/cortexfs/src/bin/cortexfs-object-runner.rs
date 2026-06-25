use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use cortexfs::{
    DEFAULT_AGENT_PROMPT_TEMPLATE, collect_agent_rules, collect_skill_metadata, current_time_unix,
    is_model_name, resolve_api_key_from_env_names, run_core_tool, run_core_tool_cli,
    run_echo_model, run_proxy_model, skill_metadata_budget_from_env,
};
use cortexfs_tool_sdk::ToolInvocation;
use serde_json::Value;

const DEFAULT_SOURCE: &str = "/var/lib/cortexfs/storage/v1-root";
const DEFAULT_CTX_ROOT: &str = "/ctx";

include!("../cortexfs_object_runner_provider.rs");

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

fn run_model(name: &str, args: &[OsString]) -> Result<(), String> {
    let name = resolve_model_name(name)?;
    if name == "debug/echo" {
        let stdout = io::stdout();
        return run_echo_model(
            args.iter().map(|value| value.to_string_lossy()),
            stdout.lock(),
        )
        .map_err(|error| format!("echo model failed: {error}"));
    }
    if name == "debug/proxy" {
        let stdout = io::stdout();
        return run_proxy_model(
            args.iter().map(|value| value.to_string_lossy()),
            stdout.lock(),
        )
        .map_err(|error| format!("proxy model failed: {error}"));
    }
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    run_provider_model(&name, &input)
}

fn resolve_model_name(name: &str) -> Result<String, String> {
    if is_model_name(name) {
        return Ok(name.to_owned());
    }
    if !is_model_alias(name) {
        return Err(format!("invalid model reference: {name}"));
    }
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    resolve_model_alias(&ctx_root, name)
}

fn resolve_model_alias(ctx_root: &Path, name: &str) -> Result<String, String> {
    let target = fs::read_link(ctx_root.join("model").join(name))
        .map_err(|_error| format!("missing model alias: {name}"))?;
    let Some(target) = target.to_str() else {
        return Err(format!("invalid model alias: {name}"));
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return Err(format!("invalid model alias target: {name}"));
    };
    if !is_model_name(model) {
        return Err(format!("invalid model alias target: {name}"));
    }
    Ok(model.to_owned())
}

fn is_model_alias(name: &str) -> bool {
    matches!(name, "main" | "helper")
}

fn resolved_model_path(ctx_root: &Path, model: &str) -> Result<PathBuf, String> {
    let name = if is_model_name(model) {
        model.to_owned()
    } else if is_model_alias(model) {
        resolve_model_alias(ctx_root, model)?
    } else {
        return Err(format!("invalid model reference: {model}"));
    };
    Ok(ctx_root.join("model").join(name))
}

fn run_provider_model(name: &str, input: &str) -> Result<(), String> {
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    write_model_start(&mut stdout, &run, name)
        .map_err(|error| format!("cannot write output: {error}"))?;
    if let Err(error) = provider_chat_completion(name, input, &run, &mut stdout) {
        write_tool_error(&mut stdout, &run, "EIO", &error)
            .map_err(|error| format!("cannot write output: {error}"))?;
        return Err(error);
    }
    write_tool_done(&mut stdout, &run, "ok")
        .map_err(|error| format!("cannot write output: {error}"))
}

fn run_agent(name: &str, args: &[OsString]) -> Result<(), String> {
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    let source =
        env::var_os("CTX_SOURCE").map_or_else(|| PathBuf::from(DEFAULT_SOURCE), PathBuf::from);
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let model = fs::read_to_string(source.join("agent").join(format!("{name}.d")).join("model"))
        .map_or_else(
            |_error| "main".to_owned(),
            |content| content.trim().to_owned(),
        );
    let model = if model.is_empty() {
        "main".to_owned()
    } else {
        model
    };
    let model_path = resolved_model_path(&ctx_root, &model)?;
    let system_prompt = fs::read_to_string(
        source
            .join("agent")
            .join(format!("{name}.d"))
            .join("system.md"),
    )
    .unwrap_or_default();
    let prompt_template = fs::read_to_string(
        source
            .join("agent")
            .join(format!("{name}.d"))
            .join("prompt.template.md"),
    )
    .unwrap_or_else(|_error| DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned());
    let rules = collect_agent_rules();
    let skills = collect_skill_metadata(skill_metadata_budget_from_env());
    let current_time_unix = current_time_unix().to_string();
    if !model_path.exists() {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        return write_tool_start(&mut stdout, &run, name)
            .and_then(|()| write_tool_error(&mut stdout, &run, "ENOENT", "missing model"))
            .map_err(|error| format!("cannot write output: {error}"));
    }
    let mut child = Command::new(model_path)
        .arg(input)
        .env("CTX_RUN_ID", &run)
        .env("CTX_AGENT", name)
        .env("CTX_AGENT_SYSTEM", system_prompt)
        .env("CTX_AGENT_PROMPT_TEMPLATE", prompt_template)
        .env("CTX_AGENT_RULES", rules)
        .env("CTX_AGENT_SKILLS", skills)
        .env("CTX_AGENT_CURRENT_TIME_UNIX", current_time_unix)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run agent model: {error}"))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read agent model output".to_owned())?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    for line in BufReader::new(child_stdout).lines() {
        let line = line.map_err(|error| format!("cannot read agent model output: {error}"))?;
        writeln!(stdout, "{line}")
            .and_then(|()| stdout.flush())
            .map_err(|error| format!("cannot write output: {error}"))?;
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot run agent model: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("agent model failed".to_owned())
    }
}

fn run_tool(name: &str, args: &[OsString]) -> Result<(), String> {
    if is_passthrough_tool(name) {
        return run_passthrough_tool(name, args);
    }
    if env::var("CTX_TOOL_MODE").as_deref() == Ok("cli") {
        return run_cli_tool(name, args);
    }
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let invocation = ToolInvocation::new(run.clone(), input);
    match run_core_tool(name, &invocation, &mut stdout) {
        Ok(true) => Ok(()),
        Ok(false) => write_tool_start(&mut stdout, &run, name)
            .and_then(|()| {
                write_tool_error(
                    &mut stdout,
                    &run,
                    "ENOSYS",
                    "tool is not implemented by cortexfs-object-runner",
                )
            })
            .map_err(|error| format!("cannot write output: {error}")),
        Err(error) => Err(format!("cannot write output: {error}")),
    }
}

fn run_cli_tool(name: &str, args: &[OsString]) -> Result<(), String> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    match run_core_tool_cli(name, args, &mut stdout) {
        Ok(Some(code)) if code == ExitCode::SUCCESS => Ok(()),
        Ok(Some(code)) => Err(format!("{name} tool exited with {code:?}")),
        Ok(None) => Err("tool is not implemented by cortexfs-object-runner".to_owned()),
        Err(error) => Err(format!("cannot run tool: {error}")),
    }
}

#[cfg(test)]
fn run_cli_tool_to_writer(
    name: &str,
    args: &[OsString],
    writer: &mut dyn Write,
) -> Result<(), String> {
    match run_core_tool_cli(name, args, writer) {
        Ok(Some(code)) if code == ExitCode::SUCCESS => Ok(()),
        Ok(Some(code)) => Err(format!("{name} tool exited with {code:?}")),
        Ok(None) => Err("tool is not implemented by cortexfs-object-runner".to_owned()),
        Err(error) => Err(format!("cannot run tool: {error}")),
    }
}

fn is_passthrough_tool(name: &str) -> bool {
    matches!(name, "bash" | "tmux" | "zellij" | "tsh")
}

fn run_passthrough_tool(name: &str, args: &[OsString]) -> Result<(), String> {
    let status = Command::new(name)
        .args(args)
        .status()
        .map_err(|error| format!("cannot run {name} tool: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} tool exited with {status}"))
    }
}

fn collect_input(args: &[OsString]) -> io::Result<String> {
    let input = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    if !input.is_empty() {
        return Ok(input);
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

fn write_model_start(stdout: &mut impl Write, run: &str, model: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"start","run":{},"model":{}}}"#,
        json_string(run),
        json_string(model)
    )
}

fn write_model_delta(stdout: &mut impl Write, run: &str, text: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"delta","run":{},"text":{}}}"#,
        json_string(run),
        json_string(text)
    )
}

fn write_tool_start(stdout: &mut impl Write, run: &str, tool: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"start","run":{},"tool":{}}}"#,
        json_string(run),
        json_string(tool)
    )
}

fn write_tool_done(stdout: &mut impl Write, run: &str, status: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"done","run":{},"status":{}}}"#,
        json_string(run),
        json_string(status)
    )
}

fn write_tool_error(
    stdout: &mut impl Write,
    run: &str,
    code: &str,
    message: &str,
) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"error","run":{},"code":{},"message":{}}}"#,
        json_string(run),
        json_string(code),
        json_string(message)
    )?;
    write_tool_done(stdout, run, "error")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_error| "\"\"".to_owned())
}

fn split_object_args(args: Vec<OsString>) -> Result<(PathBuf, Vec<OsString>), String> {
    let mut values = args.into_iter();
    let Some(path) = values.next() else {
        return Err("missing object path".to_owned());
    };
    Ok((PathBuf::from(path), values.collect()))
}

#[derive(Debug, Eq, PartialEq)]
struct ObjectPath {
    class: String,
    name: String,
}

impl ObjectPath {
    fn parse(path: &Path) -> Result<Self, String> {
        let leaf = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "object path has no valid name".to_owned())?;
        let parent = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .ok_or_else(|| "object path has no valid parent".to_owned())?;
        let (class, name) = if parent == "model" || parent == "agent" || parent == "tool" {
            (parent.to_owned(), leaf.to_owned())
        } else {
            let class = path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                .ok_or_else(|| "object path has no valid class".to_owned())?;
            (class.to_owned(), format!("{parent}/{leaf}"))
        };
        Ok(Self { class, name })
    }
}

fn write_error(line: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(line.as_bytes())
        .and_then(|()| stderr.write_all(b"\n"))
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/cortexfs_object_runner_tests.rs"
    ));
}
