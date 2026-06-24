use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use cortexfs::{resolve_api_key_from_env_names, run_echo_model};
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
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    run_provider_model(&name, &input)
}

fn resolve_model_name(name: &str) -> Result<String, String> {
    if name.contains('/') {
        return Ok(name.to_owned());
    }
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let target = fs::read_link(ctx_root.join("model").join(name))
        .map_err(|_error| format!("missing model alias: {name}"))?;
    let Some(target) = target.to_str() else {
        return Err(format!("invalid model alias: {name}"));
    };
    target
        .strip_prefix("/ctx/model/")
        .filter(|model| model.contains('/'))
        .map(str::to_owned)
        .ok_or_else(|| format!("invalid model alias target: {name}"))
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
    let model_path = ctx_root.join("model").join(&model);
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
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    match name {
        "fs.read" => run_fs_read(&run, &input, &mut stdout),
        "fs.write" => run_fs_write(&run, &input, &mut stdout),
        "shell.exec" => run_shell_exec(&run, &input, &mut stdout),
        _ => write_tool_start(&mut stdout, &run, name)
            .and_then(|()| {
                write_tool_error(
                    &mut stdout,
                    &run,
                    "ENOSYS",
                    "tool is not implemented by cortexfs-object-runner",
                )
            })
            .map_err(|error| format!("cannot write output: {error}")),
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

fn run_fs_read(run: &str, input: &str, stdout: &mut impl Write) -> Result<(), String> {
    write_tool_start(stdout, run, "fs.read")
        .map_err(|error| format!("cannot write output: {error}"))?;
    let path = json_string_field(input, "path").unwrap_or_else(|| input.trim().to_owned());
    if path.is_empty() {
        return write_tool_error(stdout, run, "EINVAL", "missing path")
            .map_err(|error| format!("cannot write output: {error}"));
    }
    match fs::read_to_string(&path) {
        Ok(content) => write_tool_message(stdout, run, &content)
            .and_then(|()| write_tool_done(stdout, run, "ok"))
            .map_err(|error| format!("cannot write output: {error}")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            write_tool_error(stdout, run, "ENOENT", "file not found")
                .map_err(|error| format!("cannot write output: {error}"))
        }
        Err(_error) => write_tool_error(stdout, run, "EACCES", "read failed")
            .map_err(|error| format!("cannot write output: {error}")),
    }
}

fn run_fs_write(run: &str, input: &str, stdout: &mut impl Write) -> Result<(), String> {
    write_tool_start(stdout, run, "fs.write")
        .map_err(|error| format!("cannot write output: {error}"))?;
    let path = json_string_field(input, "path").unwrap_or_default();
    let content = json_string_field(input, "content").unwrap_or_default();
    if path.is_empty() {
        return write_tool_error(stdout, run, "EINVAL", "missing path")
            .map_err(|error| format!("cannot write output: {error}"));
    }
    match fs::write(&path, content) {
        Ok(()) => write_tool_message(stdout, run, "written")
            .and_then(|()| write_tool_done(stdout, run, "ok"))
            .map_err(|error| format!("cannot write output: {error}")),
        Err(_error) => write_tool_error(stdout, run, "EACCES", "write failed")
            .map_err(|error| format!("cannot write output: {error}")),
    }
}

fn run_shell_exec(run: &str, input: &str, stdout: &mut impl Write) -> Result<(), String> {
    write_tool_start(stdout, run, "shell.exec")
        .map_err(|error| format!("cannot write output: {error}"))?;
    let command = json_string_field(input, "cmd").unwrap_or_else(|| input.trim().to_owned());
    if command.is_empty() {
        return write_tool_error(stdout, run, "EINVAL", "missing cmd")
            .map_err(|error| format!("cannot write output: {error}"));
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .map_err(|error| format!("cannot run shell command: {error}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    write_tool_message(stdout, run, &text)
        .map_err(|error| format!("cannot write output: {error}"))?;
    if output.status.success() {
        write_tool_done(stdout, run, "ok").map_err(|error| format!("cannot write output: {error}"))
    } else {
        write_tool_error(stdout, run, "EIO", "command failed")
            .map_err(|error| format!("cannot write output: {error}"))
    }
}

fn json_string_field(input: &str, field: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(input).ok()?;
    value.get(field)?.as_str().map(str::to_owned)
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

fn write_tool_message(stdout: &mut impl Write, run: &str, text: &str) -> io::Result<()> {
    writeln!(
        stdout,
        r#"{{"type":"message","run":{},"role":"tool","content":[{{"type":"text","text":{}}}]}}"#,
        json_string(run),
        json_string(text)
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
