use std::env;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use cortexfs::{
    AgentControlIssue, AgentControlKind, CTX_ROOT, ContextJsonlIssue, ContextJsonlKind,
    ContextPackIssue, EventStreamIssue, MAX_SOCKET_FRAME_BYTES, MessageStreamIssue,
    ModelCapabilityIssue, MountTable, ObjectClass, ObjectLayoutIssue, PolicyV0, ROOT_ENTRIES,
    SessionControlIssue, SessionControlKind, SessionIndexIssue, SessionIndexKind,
    SessionLayoutIssue, SharedQueueLayoutIssue, ToolPath, ToolSchemaIssue, classify_abi_path,
    ensure_v1_reference_tree, inspect_agent_control, inspect_context_jsonl,
    inspect_context_pack_json, inspect_event_stream_jsonl, inspect_message_stream_jsonl,
    inspect_model_capabilities, inspect_object_layout, inspect_session_control,
    inspect_session_index, inspect_session_layout, inspect_shared_queue_layout,
    inspect_tool_schema_json, is_executable_file, is_object_name,
};

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            let _ignored = write_error(&format!("ctx: {}", error.message));
            ExitCode::from(error.code)
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct CliError {
    code: u8,
    message: String,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: 69,
            message: message.into(),
        }
    }
}

#[derive(Debug)]
struct Cli {
    root: PathBuf,
    command: Command,
}

#[derive(Debug)]
enum Command {
    Help,
    Abi,
    Env,
    Root,
    Status,
    Bootstrap {
        source: Option<PathBuf>,
    },
    Mount {
        source: Option<PathBuf>,
        mountpoint: Option<PathBuf>,
    },
    Ls(LsTarget),
    Which(ObjectClass, String),
    PathShared(String),
    History {
        agent: String,
        session: Option<String>,
    },
    Latest {
        agent: String,
        session: Option<String>,
    },
    Resume {
        agent: String,
        session: Option<String>,
    },
    Send {
        agent: String,
        session: String,
        input: String,
    },
    Ping {
        path: String,
    },
    Cancel {
        path: String,
        run: String,
    },
    Doctor,
    Exec {
        path: String,
        args: Vec<String>,
    },
    File(FileArgs),
    ValidateName(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileCommand {
    Cat,
    Set,
    Append,
    Check,
    Classify,
}

#[derive(Debug, Eq, PartialEq)]
enum LsTarget {
    Root,
    Path(String),
}

#[derive(Debug)]
struct FileArgs {
    command: FileCommand,
    path: String,
    value: Option<String>,
}

fn run(args: Vec<OsString>) -> Result<ExitCode, CliError> {
    let cli = parse(args)?;
    match cli.command {
        Command::Help => success(print_help()),
        Command::Abi => success(print_abi()),
        Command::Env => success(print_env(&cli.root)),
        Command::Root => success(print_line(&cli.root.display().to_string())),
        Command::Status => success(print_status(&cli.root)),
        Command::Bootstrap { source } => success(bootstrap_reference_tree(source.as_deref())),
        Command::Mount { source, mountpoint } => success(mount_reference_tree(
            &cli.root,
            source.as_deref(),
            mountpoint.as_deref(),
        )),
        Command::Ls(target) => success(list_objects(&cli.root, &target)),
        Command::Which(class, name) => success(which_object(&cli.root, class, &name)),
        Command::PathShared(name) => success(path_shared(&cli.root, &name)),
        Command::History { agent, session } => {
            success(history(&cli.root, &agent, session.as_deref()))
        }
        Command::Latest { agent, session } => {
            success(latest(&cli.root, &agent, session.as_deref()))
        }
        Command::Resume { agent, session } => resume(&cli.root, &agent, session.as_deref()),
        Command::Send {
            agent,
            session,
            input,
        } => send(&cli.root, &agent, &session, &input),
        Command::Ping { path } => ping(&cli.root, &path),
        Command::Cancel { path, run } => cancel(&cli.root, &path, &run),
        Command::Doctor => success(doctor(&cli.root)),
        Command::Exec { path, args } => exec_object(&cli.root, &path, &args),
        Command::File(args) => success(file_command(&cli.root, &args)),
        Command::ValidateName(name) => success(validate_name(&name)),
    }
}

fn success(result: Result<(), CliError>) -> Result<ExitCode, CliError> {
    result.map(|()| ExitCode::SUCCESS)
}

fn parse(args: Vec<OsString>) -> Result<Cli, CliError> {
    let mut root = env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(CTX_ROOT), PathBuf::from);
    let mut values = args.into_iter();
    let mut rest = Vec::new();

    while let Some(value) = values.next() {
        let text = os_string(value)?;
        match text.as_str() {
            "--root" | "-r" => {
                let Some(next) = values.next() else {
                    return Err(CliError::usage("--root requires a path"));
                };
                root = PathBuf::from(next);
            }
            _ => rest.push(text),
        }
    }

    let command = parse_command(rest)?;
    Ok(Cli { root, command })
}

fn os_string(value: OsString) -> Result<String, CliError> {
    value
        .into_string()
        .map_err(|_| CliError::usage("arguments must be valid UTF-8"))
}

fn parse_command(args: Vec<String>) -> Result<Command, CliError> {
    let mut values = args.into_iter();
    let Some(command) = values.next() else {
        return Ok(Command::Status);
    };

    match command.as_str() {
        "help" | "--help" | "-h" => Ok(Command::Help),
        "abi" => Ok(Command::Abi),
        "env" => Ok(Command::Env),
        "root" => Ok(Command::Root),
        "status" => Ok(Command::Status),
        "bootstrap" => {
            let source = values.next().map(PathBuf::from);
            no_extra_args(values)?;
            Ok(Command::Bootstrap { source })
        }
        "mount" => parse_mount_command(values),
        "ls" => parse_ls_command(values),
        "which" => {
            let Some(class) = values.next() else {
                return Err(CliError::usage("which requires model, agent, or tool"));
            };
            let Some(name) = values.next() else {
                return Err(CliError::usage("which requires an object name"));
            };
            let class = ObjectClass::parse(&class)
                .ok_or_else(|| CliError::usage("which expects model, agent, or tool"))?;
            no_extra_args(values)?;
            Ok(Command::Which(class, name))
        }
        "which-tool" => {
            let Some(name) = values.next() else {
                return Err(CliError::usage("which-tool requires a tool name"));
            };
            no_extra_args(values)?;
            Ok(Command::Which(ObjectClass::Tool, name))
        }
        "path" => {
            let Some(kind) = values.next() else {
                return Err(CliError::usage("path requires a kind"));
            };
            match kind.as_str() {
                "shared" => {
                    let Some(name) = values.next() else {
                        return Err(CliError::usage("path shared requires a name"));
                    };
                    no_extra_args(values)?;
                    Ok(Command::PathShared(name))
                }
                _ => Err(CliError::usage("path expects shared")),
            }
        }
        "history" => {
            let (agent, session) = parse_agent_session(values, "history")?;
            Ok(Command::History { agent, session })
        }
        "latest" => {
            let (agent, session) = parse_agent_session(values, "latest")?;
            Ok(Command::Latest { agent, session })
        }
        "resume" => {
            let (agent, session) = parse_agent_session(values, "resume")?;
            Ok(Command::Resume { agent, session })
        }
        "send" => {
            let (agent, session, input) = parse_send(values)?;
            Ok(Command::Send {
                agent,
                session,
                input,
            })
        }
        "ping" => {
            let Some(path) = values.next() else {
                return Err(CliError::usage("ping requires model/NAME or agent/NAME"));
            };
            no_extra_args(values)?;
            Ok(Command::Ping { path })
        }
        "cancel" => {
            let Some(path) = values.next() else {
                return Err(CliError::usage("cancel requires model/NAME or agent/NAME"));
            };
            let Some(run) = values.next() else {
                return Err(CliError::usage("cancel requires a run id"));
            };
            no_extra_args(values)?;
            Ok(Command::Cancel { path, run })
        }
        "doctor" => {
            no_extra_args(values)?;
            Ok(Command::Doctor)
        }
        "exec" => {
            let Some(path) = values.next() else {
                return Err(CliError::usage("exec requires an ABI object path"));
            };
            Ok(Command::Exec {
                path,
                args: values.collect(),
            })
        }
        "file" => {
            let args = parse_file_args(values.collect())?;
            Ok(Command::File(args))
        }
        "validate-name" => {
            let Some(name) = values.next() else {
                return Err(CliError::usage("validate-name requires a name"));
            };
            no_extra_args(values)?;
            Ok(Command::ValidateName(name))
        }
        _ => Err(CliError::usage(format!("unknown command: {command}"))),
    }
}

fn parse_ls_command(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let target = values.next().map_or(LsTarget::Root, LsTarget::Path);
    no_extra_args(values)?;
    Ok(Command::Ls(target))
}

fn parse_mount_command(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut source = None;
    let mut mountpoint = None;

    while let Some(value) = values.next() {
        match value.as_str() {
            "--source" | "-s" => {
                let Some(next) = values.next() else {
                    return Err(CliError::usage("mount --source requires a path"));
                };
                source = Some(PathBuf::from(next));
            }
            _ => {
                if mountpoint.is_some() {
                    return Err(CliError::usage(format!("unexpected argument: {value}")));
                }
                mountpoint = Some(PathBuf::from(value));
            }
        }
    }

    Ok(Command::Mount { source, mountpoint })
}

fn parse_agent_session(
    mut values: impl Iterator<Item = String>,
    command: &str,
) -> Result<(String, Option<String>), CliError> {
    let Some(agent) = values.next() else {
        return Err(CliError::usage(format!("{command} requires an agent name")));
    };
    let session = values.next();
    no_extra_args(values)?;
    Ok((agent, session))
}

fn parse_send(
    mut values: impl Iterator<Item = String>,
) -> Result<(String, String, String), CliError> {
    let Some(agent) = values.next() else {
        return Err(CliError::usage("send requires an agent name"));
    };
    let Some(session) = values.next() else {
        return Err(CliError::usage("send requires a session name"));
    };
    let Some(input) = values.next() else {
        return Err(CliError::usage("send requires input text"));
    };
    no_extra_args(values)?;
    Ok((agent, session, input))
}

fn parse_file_args(args: Vec<String>) -> Result<FileArgs, CliError> {
    let mut values = args.into_iter();
    let Some(first) = values.next() else {
        return Err(CliError::usage("file requires a path or subcommand"));
    };

    let parsed = match first.as_str() {
        "cat" => {
            let Some(path) = values.next() else {
                return Err(CliError::usage("file cat requires a path"));
            };
            no_extra_args(values)?;
            FileArgs {
                command: FileCommand::Cat,
                path,
                value: None,
            }
        }
        "set" => {
            let Some(path) = values.next() else {
                return Err(CliError::usage("file set requires a path"));
            };
            let Some(value) = values.next() else {
                return Err(CliError::usage("file set requires a value"));
            };
            no_extra_args(values)?;
            FileArgs {
                command: FileCommand::Set,
                path,
                value: Some(value),
            }
        }
        "append" => {
            let Some(path) = values.next() else {
                return Err(CliError::usage("file append requires a path"));
            };
            let Some(value) = values.next() else {
                return Err(CliError::usage("file append requires a value"));
            };
            no_extra_args(values)?;
            FileArgs {
                command: FileCommand::Append,
                path,
                value: Some(value),
            }
        }
        "check" => {
            let Some(path) = values.next() else {
                return Err(CliError::usage("file check requires a path"));
            };
            no_extra_args(values)?;
            FileArgs {
                command: FileCommand::Check,
                path,
                value: None,
            }
        }
        "classify" => {
            let Some(path) = values.next() else {
                return Err(CliError::usage("file classify requires a path"));
            };
            no_extra_args(values)?;
            FileArgs {
                command: FileCommand::Classify,
                path,
                value: None,
            }
        }
        _ => {
            no_extra_args(values)?;
            FileArgs {
                command: FileCommand::Classify,
                path: first,
                value: None,
            }
        }
    };

    Ok(parsed)
}

fn no_extra_args(mut values: impl Iterator<Item = String>) -> Result<(), CliError> {
    values.next().map_or(Ok(()), |value| {
        Err(CliError::usage(format!("unexpected argument: {value}")))
    })
}

fn print_help() -> Result<(), CliError> {
    print_lines(&[
        "ctx - CortexFS filesystem management CLI",
        "",
        "usage:",
        "  ctx [--root PATH] status",
        "  ctx [--root PATH] abi",
        "  ctx [--root PATH] env",
        "  ctx [--root PATH] root",
        "  ctx bootstrap [SOURCE]",
        "  ctx [--root PATH] mount [--source SOURCE] [MOUNTPOINT]",
        "  ctx [--root PATH] ls [PATH|model|agent|tool]",
        "  ctx [--root PATH] which model|agent|tool NAME",
        "  ctx [--root PATH] path shared NAME",
        "  ctx [--root PATH] history AGENT [SESSION]",
        "  ctx [--root PATH] latest AGENT [SESSION]",
        "  ctx [--root PATH] resume AGENT [SESSION]",
        "  ctx [--root PATH] send AGENT SESSION INPUT",
        "  ctx [--root PATH] ping model/NAME|agent/NAME",
        "  ctx [--root PATH] cancel model/NAME|agent/NAME RUN",
        "  ctx [--root PATH] exec model/NAME|agent/NAME|tool/NAME [ARG...]",
        "  ctx [--root PATH] file PATH",
        "  ctx [--root PATH] file cat PATH",
        "  ctx [--root PATH] file set PATH VALUE",
        "  ctx [--root PATH] file append PATH VALUE",
        "  ctx [--root PATH] file check PATH",
        "  ctx [--root PATH] file classify PATH",
        "  ctx [--root PATH] doctor",
        "  ctx validate-name NAME",
        "",
        "principles:",
        "  ctx is a thin Unix client over /ctx",
        "  ctx does not manage providers, API formats, or private sessions",
    ])
}

fn print_abi() -> Result<(), CliError> {
    print_line("root=/ctx")?;
    print_line("entries=status bin model agent tool home shared")?;
    print_line("exec=model agent tool")?;
    print_line("socket=name.sock")?;
    print_line("control=name.d")?;
    print_line("policy=allow <subject_type> <object_class>:<object_name> <permission>")
}

fn print_env(root: &Path) -> Result<(), CliError> {
    let home = env::var("CTX_HOME").unwrap_or_else(|_| format!("{}/home/$(id -u)", root.display()));
    let path =
        env::var("CTX_PATH").unwrap_or_else(|_| format!("{}/tool:{home}/tool", root.display()));
    print_line(&format!(
        "export CTX_ROOT={}",
        shell_quote(&root.display().to_string())
    ))?;
    print_line(&format!("export CTX_HOME={}", shell_quote(&home)))?;
    print_line(&format!("export CTX_PATH={}", shell_quote(&path)))?;
    print_line(&format!("export PATH={}/bin:$PATH", root.display()))
}

fn print_status(root: &Path) -> Result<(), CliError> {
    let exists = root.exists();
    let is_dir = root.is_dir();
    let mounted = is_mount_point(root).unwrap_or(false);

    print_line(&format!("root={}", root.display()))?;
    print_line(&format!("exists={}", bool_text(exists)))?;
    print_line(&format!("dir={}", bool_text(is_dir)))?;
    print_line(&format!("mounted={}", bool_text(mounted)))?;

    for entry in ROOT_ENTRIES {
        let present = root.join(entry).exists();
        print_line(&format!("{entry}={}", bool_text(present)))?;
    }

    Ok(())
}

fn bootstrap_reference_tree(source: Option<&Path>) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    ensure_v1_reference_tree(&source).map_err(|error| {
        CliError::unavailable(format!(
            "cannot bootstrap {}: {}",
            source.display(),
            error.errno()
        ))
    })?;
    print_line(&format!("source={}", source.display()))
}

fn mount_reference_tree(
    root: &Path,
    source: Option<&Path>,
    mountpoint: Option<&Path>,
) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    let mountpoint = mountpoint.unwrap_or(root);

    ensure_v1_reference_tree(&source).map_err(|error| {
        CliError::unavailable(format!(
            "cannot bootstrap {}: {}",
            source.display(),
            error.errno()
        ))
    })?;
    fs::create_dir_all(mountpoint).map_err(|error| {
        CliError::unavailable(format!(
            "cannot create mountpoint {}: {error}",
            mountpoint.display()
        ))
    })?;
    if is_mount_point(mountpoint).unwrap_or(false) {
        return Err(CliError::unavailable(format!(
            "already mounted: {}",
            mountpoint.display()
        )));
    }

    let mount_bin = cortexfs_mount_bin();
    spawn_mount_process(&mount_bin, &source, mountpoint)?;

    for _attempt in 0..20 {
        if is_mount_point(mountpoint).unwrap_or(false) {
            print_line(&format!("mounted={}", mountpoint.display()))?;
            print_line(&format!("source={}", source.display()))?;
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Err(CliError::unavailable(format!(
        "mount did not become ready: {}",
        mountpoint.display()
    )))
}

fn default_source_root() -> Result<PathBuf, CliError> {
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join("cortexfs").join("v1-root"));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("cortexfs")
            .join("v1-root"));
    }
    Err(CliError::unavailable(
        "cannot choose default source root without HOME or XDG_DATA_HOME",
    ))
}

fn cortexfs_mount_bin() -> PathBuf {
    if let Ok(current) = env::current_exe()
        && let Some(dir) = current.parent()
    {
        let sibling = dir.join("cortexfs-mount");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("cortexfs-mount")
}

fn spawn_mount_process(mount_bin: &Path, source: &Path, mountpoint: &Path) -> Result<(), CliError> {
    let mut detached = ProcessCommand::new("setsid");
    detached
        .arg("-f")
        .arg(mount_bin)
        .arg("--source")
        .arg(source)
        .arg(mountpoint);
    match spawn_null(detached) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut direct = ProcessCommand::new(mount_bin);
            direct.arg("--source").arg(source).arg(mountpoint);
            spawn_null(direct).map_err(|error| {
                CliError::unavailable(format!("cannot start {}: {error}", mount_bin.display()))
            })
        }
        Err(error) => Err(CliError::unavailable(format!(
            "cannot start {}: {error}",
            mount_bin.display()
        ))),
    }
}

fn spawn_null(mut command: ProcessCommand) -> io::Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_child| ())
}

fn list_objects(root: &Path, target: &LsTarget) -> Result<(), CliError> {
    for entry in list_names(root, target)? {
        print_line(&entry)?;
    }
    Ok(())
}

fn list_names(root: &Path, target: &LsTarget) -> Result<Vec<String>, CliError> {
    let LsPath { path, object_class } = resolve_ls_path(root, target)?;

    if let Some(kind) = object_class {
        return list_kind_names(root, kind);
    }

    read_dir_names(&path)
}

fn list_kind_names(root: &Path, kind: ObjectClass) -> Result<Vec<String>, CliError> {
    Ok(read_dir_names(&root.join(kind.as_str()))?
        .into_iter()
        .filter(|name| is_visible_object(name))
        .collect())
}

struct LsPath {
    path: PathBuf,
    object_class: Option<ObjectClass>,
}

fn resolve_ls_path(root: &Path, target: &LsTarget) -> Result<LsPath, CliError> {
    let path = match *target {
        LsTarget::Root => return Ok(root_ls_path(root)),
        LsTarget::Path(ref path) => normalized_ls_path(path),
    };

    if path.is_empty() {
        return Ok(root_ls_path(root));
    }

    let resolved = resolve_abi_path(root, &path)?;
    let abi_path = classify_input_path(root, &path)?;
    let object_class = match abi_path.as_str() {
        "model" => Some(ObjectClass::Model),
        "agent" => Some(ObjectClass::Agent),
        "tool" => Some(ObjectClass::Tool),
        _ => None,
    };

    Ok(LsPath {
        path: resolved,
        object_class,
    })
}

fn root_ls_path(root: &Path) -> LsPath {
    LsPath {
        path: root.to_path_buf(),
        object_class: None,
    }
}

fn normalized_ls_path(path: &str) -> String {
    if path == "/" {
        return String::new();
    }

    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }

    trimmed.to_owned()
}

fn is_visible_object(name: &str) -> bool {
    is_object_name(name)
}

fn read_dir_names(dir: &Path) -> Result<Vec<String>, CliError> {
    let entries = fs::read_dir(dir).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", dir.display()))
    })?;
    let mut names = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| {
            CliError::unavailable(format!("cannot read {} entry: {error}", dir.display()))
        })?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }

    names.sort();
    Ok(names)
}

fn which_object(root: &Path, class: ObjectClass, name: &str) -> Result<(), CliError> {
    if !is_object_name(name) {
        return Err(CliError::usage(format!("invalid object name: {name}")));
    }

    if class == ObjectClass::Tool {
        return which_tool(root, name);
    }

    let candidate = root.join(class.as_str()).join(name);
    if is_executable_file(&candidate) {
        return print_line(&candidate.display().to_string());
    }

    Err(CliError::unavailable(format!(
        "{} not found: {name}",
        class.as_str()
    )))
}

fn which_tool(root: &Path, name: &str) -> Result<(), CliError> {
    if !is_object_name(name) {
        return Err(CliError::usage(format!("invalid object name: {name}")));
    }

    if let Some(hit) = ctx_tool_path(root)?.find(name).map_err(tool_path_error)? {
        return print_line(&hit.path().display().to_string());
    }

    Err(CliError::unavailable(format!("tool not found: {name}")))
}

fn path_shared(root: &Path, name: &str) -> Result<(), CliError> {
    if !is_object_name(name) {
        return Err(CliError::usage(format!("invalid shared name: {name}")));
    }

    print_line(&root.join("shared").join(name).display().to_string())
}

fn history(root: &Path, agent: &str, session: Option<&str>) -> Result<(), CliError> {
    let session_dir = agent_session_dir(root, agent, session)?;
    cat_path(&session_dir.join("messages.jsonl"))
}

fn latest(root: &Path, agent: &str, session: Option<&str>) -> Result<(), CliError> {
    let session_dir = agent_session_dir(root, agent, session)?;
    cat_path(&session_dir.join("latest.md"))
}

fn resume(root: &Path, agent: &str, session: Option<&str>) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, agent, session)?;
    let request = format!(
        "{{\"op\":\"resume\",\"session\":{}}}\n",
        json_string(&session)
    );
    stream_socket_request(&agent_socket_path(root, agent)?, &request)
}

fn send(root: &Path, agent: &str, session: &str, input: &str) -> Result<ExitCode, CliError> {
    if !is_object_name(agent) {
        return Err(CliError::usage(format!("invalid agent name: {agent}")));
    }
    if !is_object_name(session) {
        return Err(CliError::usage(format!("invalid session name: {session}")));
    }

    let request = format!(
        "{{\"op\":\"send\",\"id\":{},\"session\":{},\"input\":{}}}\n",
        json_string(&request_id()?),
        json_string(session),
        json_string(input)
    );
    stream_socket_request(&agent_socket_path(root, agent)?, &request)
}

fn ping(root: &Path, path: &str) -> Result<ExitCode, CliError> {
    stream_socket_request(&object_socket_path(root, path)?, "{\"op\":\"ping\"}\n")
}

fn cancel(root: &Path, path: &str, run: &str) -> Result<ExitCode, CliError> {
    if !is_object_name(run) {
        return Err(CliError::usage(format!("invalid run id: {run}")));
    }
    let request = format!("{{\"op\":\"cancel\",\"id\":{}}}\n", json_string(run));
    stream_socket_request(&object_socket_path(root, path)?, &request)
}

fn stream_socket_request(socket: &Path, request: &str) -> Result<ExitCode, CliError> {
    if request.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(CliError::usage(format!(
            "socket request exceeds {MAX_SOCKET_FRAME_BYTES} bytes: EMSGSIZE"
        )));
    }

    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CliError::unavailable(format!("cannot connect {}: {error}", socket.display()))
    })?;
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;

    let mut stdout = io::stdout().lock();
    io::copy(&mut stream, &mut stdout)
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))?;
    Ok(ExitCode::SUCCESS)
}

fn request_id() -> Result<String, CliError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CliError::unavailable(format!("system clock before epoch: {error}")))?
        .as_millis();
    Ok(format!("ctx-{millis}"))
}

fn agent_session_dir(root: &Path, agent: &str, session: Option<&str>) -> Result<PathBuf, CliError> {
    let session = agent_session_name(root, agent, session)?;
    Ok(ctx_home(root)?
        .join("agent")
        .join(agent)
        .join("session")
        .join(session))
}

fn agent_session_name(root: &Path, agent: &str, session: Option<&str>) -> Result<String, CliError> {
    if !is_object_name(agent) {
        return Err(CliError::usage(format!("invalid agent name: {agent}")));
    }
    if let Some(session) = session
        && !is_object_name(session)
    {
        return Err(CliError::usage(format!("invalid session name: {session}")));
    }

    let session_root = ctx_home(root)?.join("agent").join(agent).join("session");
    Ok(match session {
        Some(name) => name.to_owned(),
        None => current_session_name(&session_root)?,
    })
}

fn agent_socket_path(root: &Path, agent: &str) -> Result<PathBuf, CliError> {
    if !is_object_name(agent) {
        return Err(CliError::usage(format!("invalid agent name: {agent}")));
    }
    Ok(root.join("agent").join(format!("{agent}.sock")))
}

fn object_socket_path(root: &Path, path: &str) -> Result<PathBuf, CliError> {
    let abi_path = classify_input_path(root, path)?;
    if !matches!(
        classify_abi_path(&abi_path),
        "ctx.model.exec" | "ctx.agent.exec"
    ) {
        return Err(CliError::usage(format!(
            "socket command requires model/NAME or agent/NAME: {path}"
        )));
    }

    let Some((class, name)) = abi_path.split_once('/') else {
        return Err(CliError::usage(format!("invalid object path: {path}")));
    };
    Ok(root.join(class).join(format!("{name}.sock")))
}

fn current_session_name(session_root: &Path) -> Result<String, CliError> {
    let current_path = session_root.join("index").join("current");
    match fs::read_to_string(&current_path) {
        Ok(value) => {
            let session = value.trim();
            if is_object_name(session) {
                Ok(session.to_owned())
            } else {
                Err(CliError::unavailable(format!(
                    "invalid current session in {}",
                    current_path.display()
                )))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("default".to_owned()),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot read {}: {error}",
            current_path.display()
        ))),
    }
}

fn ctx_home(root: &Path) -> Result<PathBuf, CliError> {
    if let Some(home) = env::var_os("CTX_HOME") {
        return Ok(PathBuf::from(home));
    }

    Ok(root.join("home").join(current_uid()?))
}

fn current_uid() -> Result<String, CliError> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_err(|error| CliError::unavailable(format!("cannot run id -u: {error}")))?;
    if !output.status.success() {
        return Err(CliError::unavailable("id -u failed"));
    }
    let uid = String::from_utf8(output.stdout)
        .map_err(|_error| CliError::unavailable("id -u returned non-UTF-8 output"))?;
    let uid = uid.trim();
    if uid.is_empty() {
        return Err(CliError::unavailable("id -u returned empty output"));
    }
    Ok(uid.to_owned())
}

fn doctor(root: &Path) -> Result<(), CliError> {
    let mut ok = true;
    if root.is_dir() {
        print_line(&format!("ok root {}", root.display()))?;
    } else {
        ok = false;
        print_line(&format!("missing root {}", root.display()))?;
    }

    for entry in ROOT_ENTRIES {
        let path = root.join(entry);
        let entry_shape_matches = if entry == &"status" {
            path.is_file()
        } else {
            path.is_dir()
        };
        if entry_shape_matches {
            print_line(&format!("ok {entry}"))?;
        } else {
            ok = false;
            print_line(&format!("missing {entry}"))?;
        }
    }

    if root.is_dir() {
        for entry in read_dir_names(root)? {
            if !cortexfs::is_root_entry(&entry) {
                ok = false;
                print_line(&format!("unexpected {entry}"))?;
            }
        }
    }

    ok &= doctor_objects(root)?;
    ok &= doctor_sessions(root)?;
    ok &= doctor_shared_queues(root)?;

    if ok {
        Ok(())
    } else {
        Err(CliError::unavailable("doctor found ABI problems"))
    }
}

fn doctor_objects(root: &Path) -> Result<bool, CliError> {
    let mut ok = true;
    for class in [ObjectClass::Model, ObjectClass::Agent, ObjectClass::Tool] {
        let dir = root.join(class.as_str());
        if !dir.is_dir() {
            continue;
        }
        for name in read_dir_names(&dir)? {
            if !is_object_name(&name) {
                continue;
            }
            let report = inspect_object_layout(root, class, &name);
            if report.is_ok() {
                print_line(&format!("ok {}/{}", class.as_str(), name))?;
            } else {
                ok = false;
                print_line(&format!(
                    "invalid {}/{}: {}",
                    class.as_str(),
                    name,
                    format_object_layout_issues(report.issues())
                ))?;
            }
        }
    }
    Ok(ok)
}

fn doctor_sessions(root: &Path) -> Result<bool, CliError> {
    let mut ok = true;
    for (label, session_dir) in discover_session_dirs(root)? {
        let report = inspect_session_layout(&session_dir);
        if report.is_ok() {
            print_line(&format!("ok {label}"))?;
        } else {
            ok = false;
            print_line(&format!(
                "invalid {label}: {}",
                format_session_layout_issues(report.issues())
            ))?;
        }
    }
    Ok(ok)
}

fn doctor_shared_queues(root: &Path) -> Result<bool, CliError> {
    let mut ok = true;
    let shared = root.join("shared");
    if !shared.is_dir() {
        return Ok(ok);
    }
    for space in read_dir_names(&shared)? {
        if !is_object_name(&space) {
            continue;
        }
        let queue = shared.join(&space).join("queue");
        if !queue.exists() {
            continue;
        }
        let report = inspect_shared_queue_layout(&queue);
        if report.is_ok() {
            print_line(&format!("ok shared/{space}/queue"))?;
        } else {
            ok = false;
            print_line(&format!(
                "invalid shared/{space}/queue: {}",
                format_shared_queue_layout_issues(report.issues())
            ))?;
        }
    }
    Ok(ok)
}

fn discover_session_dirs(root: &Path) -> Result<Vec<(String, PathBuf)>, CliError> {
    let mut sessions = Vec::new();
    collect_home_sessions(root, &mut sessions)?;
    collect_shared_sessions(root, &mut sessions)?;
    sessions.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(sessions)
}

fn collect_home_sessions(
    root: &Path,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    let home = root.join("home");
    if !home.is_dir() {
        return Ok(());
    }
    for uid in read_dir_names(&home)? {
        let uid_root = home.join(&uid);
        collect_agent_sessions(
            &uid_root.join("agent"),
            &format!("home/{uid}/agent"),
            sessions,
        )?;
        collect_model_sessions(
            &uid_root.join("model"),
            &format!("home/{uid}/model"),
            sessions,
        )?;
    }
    Ok(())
}

fn collect_shared_sessions(
    root: &Path,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    let shared = root.join("shared");
    if !shared.is_dir() {
        return Ok(());
    }
    for space in read_dir_names(&shared)? {
        if !is_object_name(&space) {
            continue;
        }
        let space_root = shared.join(&space);
        collect_agent_sessions(
            &space_root.join("agent"),
            &format!("shared/{space}/agent"),
            sessions,
        )?;
        collect_model_sessions(
            &space_root.join("model"),
            &format!("shared/{space}/model"),
            sessions,
        )?;
    }
    Ok(())
}

fn collect_agent_sessions(
    agent_root: &Path,
    label_prefix: &str,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    if !agent_root.is_dir() {
        return Ok(());
    }
    for agent in read_dir_names(agent_root)? {
        if !is_object_name(&agent) {
            continue;
        }
        collect_named_sessions(
            &agent_root.join(&agent).join("session"),
            &format!("{label_prefix}/{agent}/session"),
            sessions,
        )?;
    }
    Ok(())
}

fn collect_model_sessions(
    model_root: &Path,
    label_prefix: &str,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    if !model_root.is_dir() {
        return Ok(());
    }
    for model_dir in read_dir_names(model_root)? {
        let Some(model) = model_dir.strip_suffix(".d") else {
            continue;
        };
        if !is_object_name(model) {
            continue;
        }
        collect_named_sessions(
            &model_root.join(&model_dir).join("session"),
            &format!("{label_prefix}/{model_dir}/session"),
            sessions,
        )?;
    }
    Ok(())
}

fn collect_named_sessions(
    session_root: &Path,
    label_prefix: &str,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    if !session_root.is_dir() {
        return Ok(());
    }
    for session in read_dir_names(session_root)? {
        if session == "index" || !is_object_name(&session) {
            continue;
        }
        sessions.push((
            format!("{label_prefix}/{session}"),
            session_root.join(session),
        ));
    }
    Ok(())
}

fn exec_object(root: &Path, path: &str, args: &[String]) -> Result<ExitCode, CliError> {
    let abi_path = classify_input_path(root, path)?;
    if !matches!(
        classify_abi_path(&abi_path),
        "ctx.model.exec" | "ctx.agent.exec" | "ctx.tool.exec"
    ) {
        return Err(CliError::usage(format!(
            "exec requires model/NAME, agent/NAME, or tool/NAME: {path}"
        )));
    }

    let path = resolve_abi_path(root, path)?;
    if !is_executable_file(&path) {
        return Err(CliError::unavailable(format!(
            "object is not executable: {}",
            path.display()
        )));
    }

    let status = ProcessCommand::new(&path)
        .args(args)
        .status()
        .map_err(|error| {
            CliError::unavailable(format!("cannot exec {}: {error}", path.display()))
        })?;

    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(70), ExitCode::from))
}

fn file_command(root: &Path, args: &FileArgs) -> Result<(), CliError> {
    match args.command {
        FileCommand::Cat => file_cat(root, &args.path),
        FileCommand::Set => {
            let Some(value) = args.value.as_deref() else {
                return Err(CliError::usage("file set requires a value"));
            };
            file_set(root, &args.path, value)
        }
        FileCommand::Append => {
            let Some(value) = args.value.as_deref() else {
                return Err(CliError::usage("file append requires a value"));
            };
            file_append(root, &args.path, value)
        }
        FileCommand::Check => file_check(root, &args.path),
        FileCommand::Classify => file_classify(root, &args.path),
    }
}

fn file_cat(root: &Path, path: &str) -> Result<(), CliError> {
    let path = resolve_abi_path(root, path)?;
    cat_path(&path)
}

fn cat_path(path: &Path) -> Result<(), CliError> {
    let mut file = fs::File::open(path).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", path.display()))
    })?;
    let mut stdout = io::stdout().lock();
    io::copy(&mut file, &mut stdout)
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))?;
    Ok(())
}

fn file_set(root: &Path, path: &str, value: &str) -> Result<(), CliError> {
    let path = resolve_abi_path(root, path)?;
    let Some(parent) = path.parent() else {
        return Err(CliError::usage("file set requires a parent directory"));
    };
    let temp = parent.join(temp_file_name());
    let content = newline_terminated(value);

    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|error| {
            CliError::unavailable(format!("cannot create {}: {error}", temp.display()))
        })?;
    file.write_all(content.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            CliError::unavailable(format!("cannot write {}: {error}", temp.display()))
        })?;

    fs::rename(&temp, &path).map_err(|error| {
        let _ignored = fs::remove_file(&temp);
        CliError::unavailable(format!("cannot replace {}: {error}", path.display()))
    })
}

fn file_append(root: &Path, path: &str, value: &str) -> Result<(), CliError> {
    let path = resolve_abi_path(root, path)?;
    let content = newline_terminated(value);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| {
            CliError::unavailable(format!("cannot append {}: {error}", path.display()))
        })?;
    file.write_all(content.as_bytes()).map_err(|error| {
        CliError::unavailable(format!("cannot append {}: {error}", path.display()))
    })
}

fn file_classify(root: &Path, path: &str) -> Result<(), CliError> {
    let resolved = resolve_abi_path(root, path)?;
    let shape = classify_abi_path(&classify_input_path(root, path)?);
    if shape != "ctx.unknown" {
        return print_line(shape);
    }

    if fs::symlink_metadata(&resolved).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return print_line("ctx.symlink");
    }

    if resolved.exists() {
        return print_line("ctx.ordinary");
    }

    Err(CliError::unavailable(format!(
        "unknown CortexFS path: {path}"
    )))
}

fn file_check(root: &Path, path: &str) -> Result<(), CliError> {
    let resolved = resolve_abi_path(root, path)?;
    let abi_path = classify_input_path(root, path)?;
    let shape = classify_abi_path(&abi_path);
    if shape == "ctx.unknown" {
        return file_classify(root, path);
    }

    if file_check_policy_or_mount(&abi_path, &resolved)? {
        return Ok(());
    }

    if is_tool_schema_path(&abi_path) || is_shared_tool_schema_path(&abi_path) {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_tool_schema_json(&content);
        if report.is_ok() {
            return print_line("ok");
        }
        return Err(CliError::usage(format!(
            "invalid tool schema: {}",
            format_tool_schema_issues(report.issues())
        )));
    }

    if is_shared_queue_root_path(&abi_path) {
        let report = inspect_shared_queue_layout(&resolved);
        if report.is_ok() {
            return print_line("ok");
        }
        return Err(CliError::usage(format!(
            "invalid shared queue: {}",
            format_shared_queue_layout_issues(report.issues())
        )));
    }

    if is_model_capability_path(&abi_path) {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_model_capabilities(&content);
        if report.is_ok() {
            return print_line("ok");
        }
        return Err(CliError::usage(format!(
            "invalid model capabilities: {}",
            format_model_capability_issues(report.issues())
        )));
    }

    if let Some(kind) = agent_control_path_kind(&abi_path) {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_agent_control(kind, &content);
        if report.is_ok() {
            return print_line("ok");
        }
        return Err(CliError::usage(format!(
            "invalid agent control: {}",
            format_agent_control_issues(report.issues())
        )));
    }

    if let Some(kind) = session_index_path_kind(&abi_path) {
        if kind == SessionIndexKind::ByCwd
            && fs::symlink_metadata(&resolved)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(CliError::usage(
                "invalid session index: by-cwd entry is a symlink",
            ));
        }
        let content = read_file_to_string(&resolved)?;
        let report = inspect_session_index(kind, &content);
        if report.is_ok() {
            return print_line("ok");
        }
        return Err(CliError::usage(format!(
            "invalid session index: {}",
            format_session_index_issues(report.issues())
        )));
    }

    if let Some(kind) = session_control_path_kind(&abi_path) {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_session_control(kind, &content);
        if report.is_ok() {
            return print_line("ok");
        }
        return Err(CliError::usage(format!(
            "invalid session control: {}",
            format_session_control_issues(report.issues())
        )));
    }

    if file_check_jsonl_content(&abi_path, &resolved)? {
        return Ok(());
    }

    if is_context_pack_path(&abi_path) {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_context_pack_json(&content);
        if report.is_ok() {
            return print_line("ok");
        }
        return Err(CliError::usage(format!(
            "invalid context pack: {}",
            format_context_pack_issues(report.issues())
        )));
    }

    if shape == "ctx.session.dir" && is_durable_session_instance_path(&abi_path) {
        let report = inspect_session_layout(&resolved);
        if report.is_ok() {
            return print_line("ok");
        }
        return Err(CliError::usage(format!(
            "invalid session layout: {}",
            format_session_layout_issues(report.issues())
        )));
    }

    if let Some((class, name)) = executable_object_path(&abi_path) {
        let report = inspect_object_layout(root, class, name);
        if report.is_ok() {
            return print_line("ok");
        }
        return Err(CliError::usage(format!(
            "invalid object layout: {}",
            format_object_layout_issues(report.issues())
        )));
    }

    print_line(shape)
}

fn file_check_policy_or_mount(abi_path: &str, resolved: &Path) -> Result<bool, CliError> {
    if is_control_file(abi_path, "policy") {
        let content = read_file_to_string(resolved)?;
        PolicyV0::parse(&content)
            .map_err(|error| CliError::usage(format!("invalid policy: {error:?}")))?;
        print_line("ok")?;
        return Ok(true);
    }

    if is_control_file(abi_path, "mount") {
        let content = read_file_to_string(resolved)?;
        MountTable::parse(&content)
            .map_err(|error| CliError::usage(format!("invalid mount: {error:?}")))?;
        print_line("ok")?;
        return Ok(true);
    }

    Ok(false)
}

fn file_check_jsonl_content(abi_path: &str, resolved: &Path) -> Result<bool, CliError> {
    if is_session_messages_path(abi_path) {
        let content = read_file_to_string(resolved)?;
        let report = inspect_message_stream_jsonl(&content);
        if report.is_ok() {
            print_line("ok")?;
            return Ok(true);
        }
        return Err(CliError::usage(format!(
            "invalid message stream: {}",
            format_message_stream_issues(report.issues())
        )));
    }

    if is_session_events_path(abi_path) {
        let content = read_file_to_string(resolved)?;
        let report = inspect_event_stream_jsonl(&content);
        if report.is_ok() {
            print_line("ok")?;
            return Ok(true);
        }
        return Err(CliError::usage(format!(
            "invalid event stream: {}",
            format_event_stream_issues(report.issues())
        )));
    }

    if let Some(kind) = context_jsonl_path_kind(abi_path) {
        let content = read_file_to_string(resolved)?;
        let report = inspect_context_jsonl(kind, &content);
        if report.is_ok() {
            print_line("ok")?;
            return Ok(true);
        }
        return Err(CliError::usage(format!(
            "invalid context jsonl: {}",
            format_context_jsonl_issues(report.issues())
        )));
    }

    Ok(false)
}

fn is_control_file(path: &str, file_name: &str) -> bool {
    path.split('/').next_back() == Some(file_name) && path.contains(".d/")
}

fn is_model_capability_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    parts.len() == 3
        && parts.first() == Some(&"model")
        && parts
            .get(1)
            .is_some_and(|name| name.strip_suffix(".d").is_some_and(is_object_name))
        && parts.get(2) == Some(&"cap")
}

fn is_tool_schema_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    parts.len() == 3
        && parts.first() == Some(&"tool")
        && parts
            .get(1)
            .is_some_and(|name| name.strip_suffix(".d").is_some_and(is_object_name))
        && parts.get(2) == Some(&"schema")
}

fn is_shared_tool_schema_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    parts.len() == 5
        && parts.first() == Some(&"shared")
        && parts.get(2) == Some(&"tool")
        && parts.get(1).is_some_and(|space| is_object_name(space))
        && parts
            .get(3)
            .is_some_and(|name| name.strip_suffix(".d").is_some_and(is_object_name))
        && parts.get(4) == Some(&"schema")
}

fn is_shared_queue_root_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    parts.len() == 3
        && parts.first() == Some(&"shared")
        && parts.get(1).is_some_and(|space| is_object_name(space))
        && parts.get(2) == Some(&"queue")
}

fn agent_control_path_kind(path: &str) -> Option<AgentControlKind> {
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.len() != 3
        || parts.first() != Some(&"agent")
        || !parts
            .get(1)
            .and_then(|name| name.strip_suffix(".d"))
            .is_some_and(is_object_name)
    {
        return None;
    }
    parts.get(2).and_then(|file| AgentControlKind::parse(file))
}

fn is_durable_session_instance_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    durable_session_prefix_len(&parts).is_some_and(|len| {
        parts.len() == len + 1
            && parts
                .get(len)
                .is_some_and(|session| is_object_name(session))
    })
}

fn session_index_path_kind(path: &str) -> Option<SessionIndexKind> {
    let parts = path.split('/').collect::<Vec<_>>();
    if !is_agent_session_index_prefix(&parts) {
        return None;
    }

    if parts.len() == 7 && parts.get(5) == Some(&"index") && parts.get(6) == Some(&"list") {
        Some(SessionIndexKind::List)
    } else if parts.len() == 7 && parts.get(5) == Some(&"index") && parts.get(6) == Some(&"current")
    {
        Some(SessionIndexKind::Current)
    } else if parts.len() == 8
        && parts.get(5) == Some(&"index")
        && parts.get(6) == Some(&"by-cwd")
        && parts.get(7).is_some_and(|hash| !hash.is_empty())
    {
        Some(SessionIndexKind::ByCwd)
    } else {
        None
    }
}

fn is_agent_session_index_prefix(parts: &[&str]) -> bool {
    if parts.len() < 7 {
        return false;
    }

    if parts.first() == Some(&"home")
        && parts.get(2) == Some(&"agent")
        && parts.get(4) == Some(&"session")
        && parts.get(5) == Some(&"index")
    {
        return parts.get(1).is_some_and(|uid| !uid.is_empty())
            && parts.get(3).is_some_and(|agent| is_object_name(agent));
    }

    parts.first() == Some(&"shared")
        && parts.get(2) == Some(&"agent")
        && parts.get(4) == Some(&"session")
        && parts.get(5) == Some(&"index")
        && parts.get(1).is_some_and(|space| is_object_name(space))
        && parts.get(3).is_some_and(|agent| is_object_name(agent))
}

fn is_session_events_path(path: &str) -> bool {
    is_durable_session_file_path(path, "events.jsonl")
}

fn is_session_messages_path(path: &str) -> bool {
    is_durable_session_file_path(path, "messages.jsonl")
}

fn is_durable_session_file_path(path: &str, file_name: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    durable_session_prefix_len(&parts).is_some_and(|len| {
        parts.len() == len + 2
            && parts
                .get(len)
                .is_some_and(|session| is_object_name(session))
            && parts.get(len + 1) == Some(&file_name)
    })
}

fn session_control_path_kind(path: &str) -> Option<SessionControlKind> {
    let parts = path.split('/').collect::<Vec<_>>();
    durable_session_prefix_len(&parts).and_then(|len| {
        if parts.len() == len + 2
            && parts
                .get(len)
                .is_some_and(|session| is_object_name(session))
        {
            parts
                .get(len + 1)
                .and_then(|file| SessionControlKind::parse(file))
        } else {
            None
        }
    })
}

fn durable_session_prefix_len(parts: &[&str]) -> Option<usize> {
    if parts.first() == Some(&"home")
        && parts.get(2) == Some(&"agent")
        && parts.get(4) == Some(&"session")
    {
        return (parts.get(1).is_some_and(|uid| !uid.is_empty())
            && parts.get(3).is_some_and(|agent| is_object_name(agent)))
        .then_some(5);
    }

    if parts.first() == Some(&"shared")
        && parts.get(2) == Some(&"agent")
        && parts.get(4) == Some(&"session")
    {
        return (parts.get(1).is_some_and(|space| is_object_name(space))
            && parts.get(3).is_some_and(|agent| is_object_name(agent)))
        .then_some(5);
    }

    if parts.first() == Some(&"home")
        && parts.get(2) == Some(&"model")
        && parts.get(4) == Some(&"session")
    {
        return (parts.get(1).is_some_and(|uid| !uid.is_empty())
            && parts
                .get(3)
                .and_then(|model| model.strip_suffix(".d"))
                .is_some_and(is_object_name))
        .then_some(5);
    }

    if parts.first() == Some(&"shared")
        && parts.get(2) == Some(&"model")
        && parts.get(4) == Some(&"session")
    {
        return (parts.get(1).is_some_and(|space| is_object_name(space))
            && parts
                .get(3)
                .and_then(|model| model.strip_suffix(".d"))
                .is_some_and(is_object_name))
        .then_some(5);
    }

    None
}

fn is_context_pack_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.len() != 8 {
        return false;
    }

    if parts.first() == Some(&"home")
        && parts.get(2) == Some(&"agent")
        && parts.get(4) == Some(&"session")
        && parts.get(6) == Some(&"context")
        && parts.get(7) == Some(&"pack.json")
    {
        return parts.get(3).is_some_and(|agent| is_object_name(agent))
            && parts.get(5).is_some_and(|session| is_object_name(session));
    }

    if parts.first() == Some(&"shared")
        && parts.get(2) == Some(&"agent")
        && parts.get(4) == Some(&"session")
        && parts.get(6) == Some(&"context")
        && parts.get(7) == Some(&"pack.json")
    {
        return parts.get(1).is_some_and(|space| is_object_name(space))
            && parts.get(3).is_some_and(|agent| is_object_name(agent))
            && parts.get(5).is_some_and(|session| is_object_name(session));
    }

    false
}

fn context_jsonl_path_kind(path: &str) -> Option<ContextJsonlKind> {
    let parts = path.split('/').collect::<Vec<_>>();
    durable_session_prefix_len(&parts).and_then(|len| {
        if !parts
            .get(len)
            .is_some_and(|session| is_object_name(session))
            || parts.get(len + 1) != Some(&"context")
        {
            return None;
        }
        let rest = parts.get(len + 2..)?;
        match *rest {
            ["facts.jsonl"] => Some(ContextJsonlKind::Facts),
            ["decisions.jsonl"] => Some(ContextJsonlKind::Decisions),
            ["refs.jsonl"] => Some(ContextJsonlKind::Refs),
            ["swap", "index.jsonl"] => Some(ContextJsonlKind::SwapIndex),
            ["dedup", "index.jsonl"] => Some(ContextJsonlKind::DedupIndex),
            _ => None,
        }
    })
}

fn executable_object_path(path: &str) -> Option<(ObjectClass, &str)> {
    let (class, name) = path.split_once('/')?;
    if name.contains('/') {
        return None;
    }
    let class = ObjectClass::parse(class)?;
    if is_object_name(name) {
        Some((class, name))
    } else {
        None
    }
}

fn format_message_stream_issues(issues: &[MessageStreamIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match *issue {
            MessageStreamIssue::InvalidJson(line) => write!(output, "invalid json line {line}"),
            MessageStreamIssue::MessageNotObject(line) => {
                write!(output, "message not object line {line}")
            }
            MessageStreamIssue::MissingRole(line) => write!(output, "missing role line {line}"),
            MessageStreamIssue::InvalidRole { line, ref role } => {
                write!(output, "invalid role line {line} {role}")
            }
            MessageStreamIssue::MissingContent(line) => {
                write!(output, "missing content line {line}")
            }
            MessageStreamIssue::InvalidContent(line) => {
                write!(output, "invalid content line {line}")
            }
            MessageStreamIssue::ProviderNativeField { line, ref field } => {
                write!(output, "provider native field line {line} {field}")
            }
        };
    }
    output
}

fn format_context_jsonl_issues(issues: &[ContextJsonlIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match *issue {
            ContextJsonlIssue::InvalidJson(line) => write!(output, "invalid json line {line}"),
            ContextJsonlIssue::RecordNotObject(line) => {
                write!(output, "record not object line {line}")
            }
            ContextJsonlIssue::MissingStringField { line, ref field } => {
                write!(output, "missing string field line {line} {field}")
            }
            ContextJsonlIssue::MissingNumberField { line, ref field } => {
                write!(output, "missing number field line {line} {field}")
            }
            ContextJsonlIssue::MissingStringArrayField { line, ref field } => {
                write!(output, "missing string array field line {line} {field}")
            }
            ContextJsonlIssue::InvalidField {
                line,
                ref field,
                ref value,
            } => write!(output, "invalid field line {line} {field}={value}"),
        };
    }
    output
}

fn format_event_stream_issues(issues: &[EventStreamIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match *issue {
            EventStreamIssue::InvalidJson(line) => write!(output, "invalid json line {line}"),
            EventStreamIssue::EventNotObject(line) => {
                write!(output, "event not object line {line}")
            }
            EventStreamIssue::MissingType(line) => write!(output, "missing type line {line}"),
            EventStreamIssue::UnknownType {
                line,
                ref event_type,
            } => {
                write!(output, "unknown type line {line} {event_type}")
            }
            EventStreamIssue::MissingRun(line) => write!(output, "missing run line {line}"),
            EventStreamIssue::ProviderNativeField { line, ref field } => {
                write!(output, "provider native field line {line} {field}")
            }
            EventStreamIssue::InvalidErrorCode(line) => {
                write!(output, "invalid error code line {line}")
            }
            EventStreamIssue::InvalidDoneStatus(line) => {
                write!(output, "invalid done status line {line}")
            }
            EventStreamIssue::InvalidUsage(line) => write!(output, "invalid usage line {line}"),
            EventStreamIssue::InvalidToolCall(line) => {
                write!(output, "invalid tool call line {line}")
            }
            EventStreamIssue::InvalidAgentLifecycle(line) => {
                write!(output, "invalid agent lifecycle line {line}")
            }
        };
    }
    output
}

fn format_context_pack_issues(issues: &[ContextPackIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match (issue.item(), issue.source(), issue.source_reason()) {
            (Some(item), Some(source), Some(reason)) => {
                write!(
                    output,
                    "{} item {item} {source} ({})",
                    issue.kind(),
                    reason.as_str()
                )
            }
            (Some(item), None, None) => write!(output, "{} item {item}", issue.kind()),
            _ => write!(output, "{}", issue.kind()),
        };
    }
    output
}

fn format_session_index_issues(issues: &[SessionIndexIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match *issue {
            SessionIndexIssue::EmptyValue { line } => write!(output, "empty value line {line}"),
            SessionIndexIssue::MultipleValues { line } => {
                write!(output, "multiple values line {line}")
            }
            SessionIndexIssue::InvalidSessionName { line, ref value } => {
                write!(output, "invalid session name line {line} {value}")
            }
        };
    }
    output
}

fn format_agent_control_issues(issues: &[AgentControlIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match *issue {
            AgentControlIssue::EmptyValue => write!(output, "empty value"),
            AgentControlIssue::MultipleValues { line } => {
                write!(output, "multiple values line {line}")
            }
            AgentControlIssue::InvalidNumber { line, ref value } => {
                write!(output, "invalid number line {line} {value}")
            }
            AgentControlIssue::InvalidValue { line, ref value } => {
                write!(output, "invalid value line {line} {value}")
            }
        };
    }
    output
}

fn format_session_control_issues(issues: &[SessionControlIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match *issue {
            SessionControlIssue::EmptyValue => write!(output, "empty value"),
            SessionControlIssue::MultipleValues { line } => {
                write!(output, "multiple values line {line}")
            }
            SessionControlIssue::InvalidValue { line, ref value } => {
                write!(output, "invalid value line {line} {value}")
            }
            SessionControlIssue::InvalidJson => write!(output, "invalid json"),
            SessionControlIssue::NotObject => write!(output, "not object"),
        };
    }
    output
}

fn format_object_layout_issues(issues: &[ObjectLayoutIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        if let Some(value) = issue.value() {
            let _ignored = write!(output, "{} {}={value}", issue.kind(), issue.path());
        } else {
            let _ignored = write!(output, "{} {}", issue.kind(), issue.path());
        }
    }
    output
}

fn format_session_layout_issues(issues: &[SessionLayoutIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        if let Some(value) = issue.value() {
            let _ignored = write!(output, "{} {}={value}", issue.kind(), issue.path());
        } else {
            let _ignored = write!(output, "{} {}", issue.kind(), issue.path());
        }
    }
    output
}

fn format_shared_queue_layout_issues(issues: &[SharedQueueLayoutIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match *issue {
            SharedQueueLayoutIssue::MissingDirectory(ref path) => {
                write!(output, "missing directory {path}")
            }
            SharedQueueLayoutIssue::NotDirectory(ref path) => {
                write!(output, "not directory {path}")
            }
        };
    }
    output
}

fn format_model_capability_issues(issues: &[ModelCapabilityIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match *issue {
            ModelCapabilityIssue::ProviderPrivate {
                line,
                ref capability,
            } => write!(
                output,
                "provider private capability line {line} {capability}"
            ),
            ModelCapabilityIssue::Unknown {
                line,
                ref capability,
            } => write!(output, "unknown capability line {line} {capability}"),
        };
    }
    output
}

fn format_tool_schema_issues(issues: &[ToolSchemaIssue]) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        let _ignored = match *issue {
            ToolSchemaIssue::InvalidJson => write!(output, "invalid json"),
            ToolSchemaIssue::NotObject => write!(output, "not object"),
            ToolSchemaIssue::AuthorityField(ref field) => {
                write!(output, "authority field {field}")
            }
        };
    }
    output
}

fn read_file_to_string(path: &Path) -> Result<String, CliError> {
    fs::read_to_string(path)
        .map_err(|error| CliError::unavailable(format!("cannot read {}: {error}", path.display())))
}

fn classify_input_path(root: &Path, path: &str) -> Result<String, CliError> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return candidate
            .strip_prefix(root)
            .map(|relative| relative.display().to_string())
            .map_err(|_| CliError::usage("absolute file path must be under CTX_ROOT"));
    }
    Ok(path.to_owned())
}

fn resolve_abi_path(root: &Path, path: &str) -> Result<PathBuf, CliError> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return candidate
            .strip_prefix(root)
            .map(|relative| root.join(relative))
            .map_err(|_| CliError::usage("absolute file path must be under CTX_ROOT"));
    }

    if path
        .split('/')
        .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(CliError::usage("file path must be a relative ABI path"));
    }

    Ok(root.join(path))
}

fn newline_terminated(value: &str) -> String {
    if value.ends_with('\n') {
        value.to_owned()
    } else {
        format!("{value}\n")
    }
}

fn json_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                let _ignored = write!(escaped, "\\u{:04x}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn temp_file_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".ctx.tmp.{}.{}", std::process::id(), nanos)
}

fn validate_name(name: &str) -> Result<(), CliError> {
    if is_object_name(name) {
        print_line("ok")
    } else {
        Err(CliError::usage(format!("invalid name: {name}")))
    }
}

fn ctx_tool_path(root: &Path) -> Result<ToolPath, CliError> {
    match env::var("CTX_PATH") {
        Ok(value) => Ok(ToolPath::parse(&value)),
        Err(env::VarError::NotPresent) => Ok(ToolPath::default(root, &ctx_home(root)?)),
        Err(env::VarError::NotUnicode(_value)) => {
            Err(CliError::usage("CTX_PATH must be valid UTF-8"))
        }
    }
}

fn tool_path_error(error: cortexfs::ToolPathError) -> CliError {
    match error {
        cortexfs::ToolPathError::InvalidName => CliError::usage("invalid tool name"),
        cortexfs::ToolPathError::CannotReadDirectory => {
            CliError::unavailable("cannot read CTX_PATH directory")
        }
    }
}

fn is_mount_point(root: &Path) -> io::Result<bool> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
    let root = root.display().to_string();
    Ok(mountinfo
        .lines()
        .any(|line| mount_point(line).is_some_and(|point| point == root)))
}

fn mount_point(line: &str) -> Option<String> {
    let mut fields = line.split(' ');
    let _id = fields.next()?;
    let _parent = fields.next()?;
    let _major_minor = fields.next()?;
    let _root = fields.next()?;
    fields.next().map(unescape_mountinfo)
}

fn unescape_mountinfo(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn bool_text(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

fn shell_quote(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':')
    }) {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn print_lines(lines: &[&str]) -> Result<(), CliError> {
    for line in lines {
        print_line(line)?;
    }
    Ok(())
}

fn print_line(line: &str) -> Result<(), CliError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(line.as_bytes())
        .and_then(|()| stdout.write_all(b"\n"))
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))
}

fn write_error(line: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(line.as_bytes())
        .and_then(|()| stderr.write_all(b"\n"))
}

#[cfg(test)]
mod tests {
    use super::{
        Command, FileCommand, LsTarget, MAX_SOCKET_FRAME_BYTES, ObjectClass,
        agent_control_path_kind, context_jsonl_path_kind, doctor, executable_object_path,
        file_check, format_agent_control_issues, format_context_jsonl_issues,
        format_context_pack_issues, format_event_stream_issues, format_message_stream_issues,
        format_model_capability_issues, format_object_layout_issues, format_session_control_issues,
        format_session_index_issues, format_session_layout_issues,
        format_shared_queue_layout_issues, format_tool_schema_issues, is_context_pack_path,
        is_durable_session_instance_path, is_model_capability_path, is_session_events_path,
        is_session_messages_path, is_shared_queue_root_path, is_shared_tool_schema_path,
        is_tool_schema_path, json_string, list_names, newline_terminated, parse_command,
        resolve_abi_path, session_control_path_kind, session_index_path_kind,
        stream_socket_request,
    };
    use cortexfs::{
        AgentControlIssue, AgentControlKind, CHILD_RESULT_REQUIRED_FILES, CONTEXT_REQUIRED_DIRS,
        CONTEXT_REQUIRED_FILES, ContextJsonlIssue, ContextJsonlKind, ContextPackIssue,
        ContextPackSourceError, EventStreamIssue, MessageStreamIssue, ModelCapabilityIssue,
        ObjectLayoutIssue, SESSION_REQUIRED_FILES, SessionControlIssue, SessionControlKind,
        SessionIndexIssue, SessionIndexKind, SessionLayoutIssue, SharedQueueLayoutIssue,
        ToolSchemaIssue, ensure_v1_reference_tree,
    };
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_spec_which_command() {
        let command = parse_command(vec![
            "which".to_owned(),
            "tool".to_owned(),
            "fs.read".to_owned(),
        ]);
        assert!(matches!(
            command,
            Ok(Command::Which(ObjectClass::Tool, ref name)) if name == "fs.read"
        ));
    }

    #[test]
    fn parses_file_set_command() {
        let command = parse_command(vec![
            "file".to_owned(),
            "set".to_owned(),
            "agent/coder.d/cwd".to_owned(),
            "/work".to_owned(),
        ]);
        assert!(matches!(
            command,
            Ok(Command::File(ref args))
                if args.command == FileCommand::Set
                    && args.path == "agent/coder.d/cwd"
                    && args.value.as_deref() == Some("/work")
        ));
    }

    #[test]
    fn parses_file_classify_command() {
        let explicit = parse_command(vec![
            "file".to_owned(),
            "classify".to_owned(),
            "tool/fs.read".to_owned(),
        ]);
        assert!(matches!(
            explicit,
            Ok(Command::File(ref args))
                if args.command == FileCommand::Classify
                    && args.path == "tool/fs.read"
                    && args.value.is_none()
        ));

        let shorthand = parse_command(vec!["file".to_owned(), "tool/fs.read".to_owned()]);
        assert!(matches!(
            shorthand,
            Ok(Command::File(ref args))
                if args.command == FileCommand::Classify
                    && args.path == "tool/fs.read"
                    && args.value.is_none()
        ));
    }

    #[test]
    fn parses_ls_path_command() {
        let root = parse_command(vec!["ls".to_owned()]);
        assert!(matches!(root, Ok(Command::Ls(LsTarget::Root))));

        let home = parse_command(vec!["ls".to_owned(), "home".to_owned()]);
        assert!(matches!(
            home,
            Ok(Command::Ls(LsTarget::Path(ref path))) if path == "home"
        ));

        let tool = parse_command(vec!["ls".to_owned(), "tool".to_owned()]);
        assert!(matches!(
            tool,
            Ok(Command::Ls(LsTarget::Path(ref path))) if path == "tool"
        ));
    }

    #[test]
    fn parses_session_file_commands() {
        let history = parse_command(vec!["history".to_owned(), "coder".to_owned()]);
        assert!(matches!(
            history,
            Ok(Command::History {
                ref agent,
                session: None
            }) if agent == "coder"
        ));

        let latest = parse_command(vec![
            "latest".to_owned(),
            "coder".to_owned(),
            "default".to_owned(),
        ]);
        assert!(matches!(
            latest,
            Ok(Command::Latest {
                ref agent,
                session: Some(ref session)
            }) if agent == "coder" && session == "default"
        ));

        let resume = parse_command(vec![
            "resume".to_owned(),
            "coder".to_owned(),
            "default".to_owned(),
        ]);
        assert!(matches!(
            resume,
            Ok(Command::Resume {
                ref agent,
                session: Some(ref session)
            }) if agent == "coder" && session == "default"
        ));

        let send = parse_command(vec![
            "send".to_owned(),
            "coder".to_owned(),
            "default".to_owned(),
            "hello".to_owned(),
        ]);
        assert!(matches!(
            send,
            Ok(Command::Send {
                ref agent,
                ref session,
                ref input
            }) if agent == "coder" && session == "default" && input == "hello"
        ));

        let ping = parse_command(vec!["ping".to_owned(), "agent/coder".to_owned()]);
        assert!(matches!(
            ping,
            Ok(Command::Ping { ref path }) if path == "agent/coder"
        ));

        let cancel = parse_command(vec![
            "cancel".to_owned(),
            "agent/coder".to_owned(),
            "run-1".to_owned(),
        ]);
        assert!(matches!(
            cancel,
            Ok(Command::Cancel { ref path, ref run }) if path == "agent/coder" && run == "run-1"
        ));
    }

    #[test]
    fn parses_bootstrap_and_mount_commands() {
        let bootstrap = parse_command(vec!["bootstrap".to_owned()]);
        assert!(matches!(bootstrap, Ok(Command::Bootstrap { source: None })));

        let bootstrap_source = parse_command(vec![
            "bootstrap".to_owned(),
            "/tmp/cortexfs-source".to_owned(),
        ]);
        assert!(matches!(
            bootstrap_source,
            Ok(Command::Bootstrap {
                source: Some(ref source)
            }) if source == Path::new("/tmp/cortexfs-source")
        ));

        let mount = parse_command(vec![
            "mount".to_owned(),
            "--source".to_owned(),
            "/tmp/cortexfs-source".to_owned(),
            "/tmp/cortexfs-mount".to_owned(),
        ]);
        assert!(matches!(
            mount,
            Ok(Command::Mount {
                source: Some(ref source),
                mountpoint: Some(ref mountpoint)
            }) if source == Path::new("/tmp/cortexfs-source")
                && mountpoint == Path::new("/tmp/cortexfs-mount")
        ));
    }

    #[test]
    fn parses_exec_command_with_arguments() {
        let command = parse_command(vec![
            "exec".to_owned(),
            "agent/coder".to_owned(),
            "fix tests".to_owned(),
        ]);
        assert!(matches!(
            command,
            Ok(Command::Exec {
                ref path,
                ref args
            }) if path == "agent/coder" && args == &["fix tests".to_owned()]
        ));
    }

    #[test]
    fn abi_path_resolution_rejects_escape() {
        let root = Path::new("/ctx");
        assert!(resolve_abi_path(root, "agent/coder.d/cwd").is_ok());
        assert!(resolve_abi_path(root, "../etc/passwd").is_err());
        assert!(resolve_abi_path(root, "/etc/passwd").is_err());
        assert_eq!(
            resolve_abi_path(root, "/ctx/agent/coder.d/cwd").map(|path| path.display().to_string()),
            Ok("/ctx/agent/coder.d/cwd".to_owned())
        );
    }

    #[test]
    fn ls_lists_abi_paths_and_keeps_object_filtering() {
        let root = unique_test_dir("ctx-ls-paths");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(ensure_v1_reference_tree(&root).is_ok());

        let home = list_names(&root, &LsTarget::Path("home".to_owned()));
        assert_eq!(home, Ok(vec!["1000".to_owned()]));

        let root_alias = list_names(&root, &LsTarget::Path("/".to_owned()));
        assert!(matches!(root_alias, Ok(ref names) if names.contains(&"home".to_owned())));

        let absolute_home = root.join("home");
        let absolute_home = absolute_home.display().to_string();
        let home_absolute = list_names(&root, &LsTarget::Path(absolute_home));
        assert_eq!(home_absolute, Ok(vec!["1000".to_owned()]));

        let tool = list_names(&root, &LsTarget::Path("tool".to_owned()));
        assert!(matches!(
            tool,
            Ok(ref names)
                if names.contains(&"fs.read".to_owned())
                    && !names.contains(&"fs.read.d".to_owned())
        ));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_durable_session_instance_paths() {
        assert!(is_durable_session_instance_path(
            "home/1000/agent/coder/session/default"
        ));
        assert!(is_durable_session_instance_path(
            "shared/im-qq-dev/agent/bot/session/group-456"
        ));
        assert!(is_durable_session_instance_path(
            "home/1000/model/qwen.d/session/default"
        ));
        assert!(is_durable_session_instance_path(
            "shared/project-a/model/qwen.d/session/default"
        ));
        assert!(!is_durable_session_instance_path(
            "home/1000/agent/coder/session"
        ));
        assert!(!is_durable_session_instance_path(
            "home/1000/agent/coder/session/default/messages.jsonl"
        ));
        assert!(!is_durable_session_instance_path(
            "shared/project-a/model/qwen/session/default"
        ));
    }

    #[test]
    fn detects_session_control_paths() {
        assert_eq!(
            session_control_path_kind("home/1000/agent/coder/session/default/state"),
            Some(SessionControlKind::State)
        );
        assert_eq!(
            session_control_path_kind("shared/im-qq-dev/agent/bot/session/group-456/cwd"),
            Some(SessionControlKind::Cwd)
        );
        assert_eq!(
            session_control_path_kind("home/1000/model/qwen.d/session/default/meta.json"),
            Some(SessionControlKind::MetaJson)
        );
        assert_eq!(
            session_control_path_kind("home/1000/agent/coder/session/default/messages.jsonl"),
            None
        );
    }

    #[test]
    fn detects_private_and_shared_context_pack_paths() {
        assert!(is_context_pack_path(
            "home/1000/agent/coder/session/default/context/pack.json"
        ));
        assert!(is_context_pack_path(
            "shared/im-qq-dev/agent/bot/session/group-456/context/pack.json"
        ));
        assert!(!is_context_pack_path(
            "home/1000/agent/coder/session/default/context/pack.md"
        ));
        assert!(!is_context_pack_path(
            "home/1000/agent/bad/name/session/default/context/pack.json"
        ));
    }

    #[test]
    fn detects_private_and_shared_event_stream_paths() {
        assert!(is_session_events_path(
            "home/1000/agent/coder/session/default/events.jsonl"
        ));
        assert!(is_session_events_path(
            "shared/im-qq-dev/agent/bot/session/group-456/events.jsonl"
        ));
        assert!(is_session_events_path(
            "home/1000/model/qwen.d/session/default/events.jsonl"
        ));
        assert!(is_session_events_path(
            "shared/project-a/model/qwen.d/session/default/events.jsonl"
        ));
        assert!(!is_session_events_path(
            "home/1000/agent/coder/session/default/messages.jsonl"
        ));
        assert!(!is_session_events_path(
            "shared/im-qq-dev/agent/bad/name/session/group-456/events.jsonl"
        ));
    }

    #[test]
    fn detects_private_and_shared_message_stream_paths() {
        assert!(is_session_messages_path(
            "home/1000/agent/coder/session/default/messages.jsonl"
        ));
        assert!(is_session_messages_path(
            "shared/im-qq-dev/agent/bot/session/group-456/messages.jsonl"
        ));
        assert!(is_session_messages_path(
            "home/1000/model/qwen.d/session/default/messages.jsonl"
        ));
        assert!(!is_session_messages_path(
            "home/1000/agent/coder/session/default/events.jsonl"
        ));
    }

    #[test]
    fn detects_context_jsonl_paths() {
        assert_eq!(
            context_jsonl_path_kind("home/1000/agent/coder/session/default/context/facts.jsonl"),
            Some(ContextJsonlKind::Facts)
        );
        assert_eq!(
            context_jsonl_path_kind(
                "shared/im-qq-dev/agent/bot/session/group-456/context/decisions.jsonl"
            ),
            Some(ContextJsonlKind::Decisions)
        );
        assert_eq!(
            context_jsonl_path_kind(
                "home/1000/model/qwen.d/session/default/context/swap/index.jsonl"
            ),
            Some(ContextJsonlKind::SwapIndex)
        );
        assert_eq!(
            context_jsonl_path_kind(
                "shared/project-a/model/qwen.d/session/default/context/dedup/index.jsonl"
            ),
            Some(ContextJsonlKind::DedupIndex)
        );
        assert_eq!(
            context_jsonl_path_kind("home/1000/agent/coder/session/default/context/pack.json"),
            None
        );
    }

    #[test]
    fn detects_private_and_shared_session_index_paths() {
        assert_eq!(
            session_index_path_kind("home/1000/agent/coder/session/index/list"),
            Some(SessionIndexKind::List)
        );
        assert_eq!(
            session_index_path_kind("home/1000/agent/coder/session/index/current"),
            Some(SessionIndexKind::Current)
        );
        assert_eq!(
            session_index_path_kind("shared/im-qq-dev/agent/bot/session/index/by-cwd/hash-1"),
            Some(SessionIndexKind::ByCwd)
        );
        assert_eq!(
            session_index_path_kind("home/1000/agent/coder/session/default"),
            None
        );
        assert_eq!(
            session_index_path_kind("home/1000/agent/bad/name/session/index/list"),
            None
        );
    }

    #[test]
    fn detects_executable_object_paths() {
        assert_eq!(
            executable_object_path("model/qwen"),
            Some((ObjectClass::Model, "qwen"))
        );
        assert_eq!(
            executable_object_path("agent/coder"),
            Some((ObjectClass::Agent, "coder"))
        );
        assert_eq!(
            executable_object_path("tool/fs.read"),
            Some((ObjectClass::Tool, "fs.read"))
        );
        assert_eq!(executable_object_path("tool/fs.read.d/schema"), None);
        assert_eq!(executable_object_path("home/1000"), None);
    }

    #[test]
    fn detects_model_capability_paths() {
        assert!(is_model_capability_path("model/qwen.d/cap"));
        assert!(is_model_capability_path("model/qwen-2.5.d/cap"));
        assert!(!is_model_capability_path("tool/fs.read.d/cap"));
        assert!(!is_model_capability_path("model/qwen/cap"));
        assert!(!is_model_capability_path("model/qwen.d/native"));
    }

    #[test]
    fn detects_tool_schema_paths() {
        assert!(is_tool_schema_path("tool/fs.read.d/schema"));
        assert!(is_tool_schema_path(
            "tool/mcp.github.search_issues.d/schema"
        ));
        assert!(!is_tool_schema_path("tool/fs.read/schema"));
        assert!(!is_tool_schema_path("model/qwen.d/schema"));
        assert!(!is_tool_schema_path("tool/bad/name.d/schema"));
    }

    #[test]
    fn detects_shared_tool_schema_paths() {
        assert!(is_shared_tool_schema_path(
            "shared/project-a/tool/project.test.d/schema"
        ));
        assert!(is_shared_tool_schema_path(
            "shared/project-a/tool/mcp.github.search_issues.d/schema"
        ));
        assert!(!is_shared_tool_schema_path(
            "shared/project-a/tool/project.test.d/policy"
        ));
        assert!(!is_shared_tool_schema_path("tool/project.test.d/schema"));
        assert!(!is_shared_tool_schema_path(
            "shared/project-a/tool/bad/name.d/schema"
        ));
    }

    #[test]
    fn detects_shared_queue_root_paths() {
        assert!(is_shared_queue_root_path("shared/project-a/queue"));
        assert!(is_shared_queue_root_path("shared/im-qq-dev/queue"));
        assert!(!is_shared_queue_root_path("shared/project-a/queue/pending"));
        assert!(!is_shared_queue_root_path("shared/project-a/result"));
        assert!(!is_shared_queue_root_path("shared/bad/name/queue"));
    }

    #[test]
    fn detects_agent_control_paths_with_fixed_value_syntax() {
        assert_eq!(
            agent_control_path_kind("agent/coder.d/uid"),
            Some(AgentControlKind::Uid)
        );
        assert_eq!(
            agent_control_path_kind("agent/coder.d/life"),
            Some(AgentControlKind::Life)
        );
        assert_eq!(
            agent_control_path_kind("agent/rev-1.d/parent"),
            Some(AgentControlKind::Parent)
        );
        assert_eq!(agent_control_path_kind("agent/coder.d/label"), None);
        assert_eq!(agent_control_path_kind("model/qwen.d/session"), None);
        assert_eq!(agent_control_path_kind("agent/bad/name.d/uid"), None);
    }

    #[test]
    fn formats_session_layout_issues_for_file_check() {
        let formatted = format_session_layout_issues(&[
            SessionLayoutIssue::MissingFile("messages.jsonl".to_owned()),
            SessionLayoutIssue::NotDirectory("context".to_owned()),
            SessionLayoutIssue::InvalidFileValue {
                path: "state".to_owned(),
                value: "running".to_owned(),
            },
        ]);
        assert_eq!(
            formatted,
            "missing file messages.jsonl, not directory context, invalid file value state=running"
        );
    }

    #[test]
    fn formats_context_pack_issues_for_file_check() {
        let formatted = format_context_pack_issues(&[
            ContextPackIssue::InvalidSource {
                item: 1,
                source: "../other/messages.jsonl".to_owned(),
                reason: ContextPackSourceError::ParentComponent,
            },
            ContextPackIssue::MissingSource(2),
            ContextPackIssue::InvalidJson,
        ]);
        assert_eq!(
            formatted,
            "invalid source item 1 ../other/messages.jsonl (parent component), missing source item 2, invalid json"
        );
    }

    #[test]
    fn formats_event_stream_issues_for_file_check() {
        let formatted = format_event_stream_issues(&[
            EventStreamIssue::ProviderNativeField {
                line: 1,
                field: "response_id".to_owned(),
            },
            EventStreamIssue::UnknownType {
                line: 2,
                event_type: "native_thread".to_owned(),
            },
            EventStreamIssue::InvalidUsage(3),
            EventStreamIssue::InvalidAgentLifecycle(4),
        ]);
        assert_eq!(
            formatted,
            "provider native field line 1 response_id, unknown type line 2 native_thread, invalid usage line 3, invalid agent lifecycle line 4"
        );
    }

    #[test]
    fn formats_message_stream_issues_for_file_check() {
        let formatted = format_message_stream_issues(&[
            MessageStreamIssue::ProviderNativeField {
                line: 1,
                field: "thread_id".to_owned(),
            },
            MessageStreamIssue::InvalidRole {
                line: 2,
                role: "developer".to_owned(),
            },
            MessageStreamIssue::InvalidContent(3),
            MessageStreamIssue::MissingContent(4),
        ]);
        assert_eq!(
            formatted,
            "provider native field line 1 thread_id, invalid role line 2 developer, invalid content line 3, missing content line 4"
        );
    }

    #[test]
    fn formats_context_jsonl_issues_for_file_check() {
        let formatted = format_context_jsonl_issues(&[
            ContextJsonlIssue::InvalidField {
                line: 1,
                field: "path".to_owned(),
                value: "../secret".to_owned(),
            },
            ContextJsonlIssue::MissingStringField {
                line: 2,
                field: "source".to_owned(),
            },
            ContextJsonlIssue::MissingNumberField {
                line: 3,
                field: "tokens".to_owned(),
            },
            ContextJsonlIssue::MissingStringArrayField {
                line: 4,
                field: "refs".to_owned(),
            },
        ]);
        assert_eq!(
            formatted,
            "invalid field line 1 path=../secret, missing string field line 2 source, missing number field line 3 tokens, missing string array field line 4 refs"
        );
    }

    #[test]
    fn formats_model_capability_issues_for_file_check() {
        let formatted = format_model_capability_issues(&[
            ModelCapabilityIssue::ProviderPrivate {
                line: 1,
                capability: "openai_responses".to_owned(),
            },
            ModelCapabilityIssue::Unknown {
                line: 2,
                capability: "vendor_magic".to_owned(),
            },
        ]);
        assert_eq!(
            formatted,
            "provider private capability line 1 openai_responses, unknown capability line 2 vendor_magic"
        );
    }

    #[test]
    fn formats_tool_schema_issues_for_file_check() {
        let formatted = format_tool_schema_issues(&[
            ToolSchemaIssue::AuthorityField("policy".to_owned()),
            ToolSchemaIssue::InvalidJson,
            ToolSchemaIssue::NotObject,
        ]);
        assert_eq!(
            formatted,
            "authority field policy, invalid json, not object"
        );
    }

    #[test]
    fn formats_session_index_issues_for_file_check() {
        let formatted = format_session_index_issues(&[
            SessionIndexIssue::InvalidSessionName {
                line: 2,
                value: "bad/name".to_owned(),
            },
            SessionIndexIssue::MultipleValues { line: 3 },
            SessionIndexIssue::EmptyValue { line: 4 },
        ]);
        assert_eq!(
            formatted,
            "invalid session name line 2 bad/name, multiple values line 3, empty value line 4"
        );
    }

    #[test]
    fn formats_agent_control_issues_for_file_check() {
        let formatted = format_agent_control_issues(&[
            AgentControlIssue::InvalidNumber {
                line: 1,
                value: "abc".to_owned(),
            },
            AgentControlIssue::InvalidValue {
                line: 2,
                value: "detached".to_owned(),
            },
            AgentControlIssue::MultipleValues { line: 3 },
            AgentControlIssue::EmptyValue,
        ]);
        assert_eq!(
            formatted,
            "invalid number line 1 abc, invalid value line 2 detached, multiple values line 3, empty value"
        );
    }

    #[test]
    fn formats_session_control_issues_for_file_check() {
        let formatted = format_session_control_issues(&[
            SessionControlIssue::InvalidValue {
                line: 1,
                value: "running".to_owned(),
            },
            SessionControlIssue::MultipleValues { line: 2 },
            SessionControlIssue::InvalidJson,
            SessionControlIssue::NotObject,
            SessionControlIssue::EmptyValue,
        ]);
        assert_eq!(
            formatted,
            "invalid value line 1 running, multiple values line 2, invalid json, not object, empty value"
        );
    }

    #[test]
    fn file_check_validates_session_control_files() {
        let root = unique_test_dir("ctx-session-control-check");
        let session = root
            .join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session")
            .join("default");
        assert!(fs::create_dir_all(&session).is_ok());
        assert!(fs::write(session.join("state"), "idle\n").is_ok());
        assert!(fs::write(session.join("cwd"), "/work\n").is_ok());
        assert!(
            fs::write(
                session.join("meta.json"),
                "{\"client\":\"ctx\",\"model\":\"qwen\",\"scope\":\"private\"}\n"
            )
            .is_ok()
        );

        assert!(file_check(&root, "home/1000/agent/coder/session/default/state").is_ok());
        assert!(file_check(&root, "home/1000/agent/coder/session/default/cwd").is_ok());
        assert!(file_check(&root, "home/1000/agent/coder/session/default/meta.json").is_ok());

        assert!(fs::write(session.join("cwd"), "../host\n").is_ok());
        let checked = file_check(&root, "home/1000/agent/coder/session/default/cwd");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid value"))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_agent_control_files() {
        let root = unique_test_dir("ctx-agent-control-check");
        let control = root.join("agent").join("coder.d");
        assert!(fs::create_dir_all(&control).is_ok());
        assert!(fs::write(control.join("uid"), "1000\n").is_ok());
        assert!(fs::write(control.join("life"), "detached\n").is_ok());
        assert!(
            fs::write(
                control.join("parent"),
                "agent:coder session:default run:r1\n"
            )
            .is_ok()
        );

        assert!(file_check(&root, "agent/coder.d/uid").is_ok());
        let checked = file_check(&root, "agent/coder.d/life");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid value"))
        );
        assert!(file_check(&root, "agent/coder.d/parent").is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_session_index_files() {
        let root = unique_test_dir("ctx-session-index-check");
        let index = root
            .join("shared")
            .join("im-qq-dev")
            .join("agent")
            .join("bot")
            .join("session")
            .join("index");
        assert!(fs::create_dir_all(index.join("by-cwd")).is_ok());
        assert!(fs::write(index.join("list"), "group-456\nbad/name\n").is_ok());
        assert!(fs::write(index.join("current"), "group-456\n").is_ok());
        assert!(fs::write(index.join("by-cwd").join("hash-1"), "group-456\n").is_ok());

        let checked = file_check(&root, "shared/im-qq-dev/agent/bot/session/index/list");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid session name"))
        );
        assert!(file_check(&root, "shared/im-qq-dev/agent/bot/session/index/current").is_ok());
        assert!(
            file_check(
                &root,
                "shared/im-qq-dev/agent/bot/session/index/by-cwd/hash-1"
            )
            .is_ok()
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_rejects_by_cwd_symlink_index_entries() {
        let root = unique_test_dir("ctx-session-index-symlink");
        let by_cwd = root
            .join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session")
            .join("index")
            .join("by-cwd");
        assert!(fs::create_dir_all(&by_cwd).is_ok());
        assert!(fs::write(by_cwd.join("target"), "default\n").is_ok());
        assert!(std::os::unix::fs::symlink("target", by_cwd.join("hash-1")).is_ok());

        let checked = file_check(&root, "home/1000/agent/coder/session/index/by-cwd/hash-1");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("by-cwd entry is a symlink"))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_model_capability_files() {
        let root = unique_test_dir("ctx-model-cap-check");
        let cap = root.join("model").join("qwen.d").join("cap");
        let parent = cap.parent();
        assert!(parent.is_some());
        let Some(parent) = parent else { return };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(fs::write(&cap, "chat\nopenai_responses\n").is_ok());

        let checked = file_check(&root, "model/qwen.d/cap");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("provider private capability"))
        );

        assert!(fs::write(&cap, "chat\nstream\n").is_ok());
        let checked = file_check(&root, "model/qwen.d/cap");
        assert!(checked.is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_tool_schema_files() {
        let root = unique_test_dir("ctx-tool-schema-check");
        let schema = root.join("tool").join("fs.read.d").join("schema");
        let parent = schema.parent();
        assert!(parent.is_some());
        let Some(parent) = parent else { return };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(
            fs::write(
                &schema,
                "{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\"}}}\n"
            )
            .is_ok()
        );
        assert!(file_check(&root, "tool/fs.read.d/schema").is_ok());

        assert!(fs::write(&schema, "{\"policy\":\"allow all\"}\n").is_ok());
        let checked = file_check(&root, "tool/fs.read.d/schema");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("authority field policy"))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_shared_tool_schema_files() {
        let root = unique_test_dir("ctx-shared-tool-schema-check");
        let schema = root
            .join("shared")
            .join("project-a")
            .join("tool")
            .join("project.test.d")
            .join("schema");
        let parent = schema.parent();
        assert!(parent.is_some());
        let Some(parent) = parent else { return };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(
            fs::write(
                &schema,
                "{\"type\":\"object\",\"properties\":{\"target\":{\"type\":\"string\"}}}\n"
            )
            .is_ok()
        );
        assert!(file_check(&root, "shared/project-a/tool/project.test.d/schema").is_ok());

        assert!(fs::write(&schema, "{\"authority\":\"local\"}\n").is_ok());
        let checked = file_check(&root, "shared/project-a/tool/project.test.d/schema");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid tool schema") && error.message.contains("authority field authority"))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_shared_queue_roots() {
        let root = unique_test_dir("ctx-shared-queue-check");
        let queue = root.join("shared").join("project-a").join("queue");
        for name in ["inbox", "pending", "lease", "claimed", "done", "failed"] {
            assert!(fs::create_dir_all(queue.join(name)).is_ok());
        }

        assert!(file_check(&root, "shared/project-a/queue").is_ok());

        assert!(fs::remove_dir(queue.join("lease")).is_ok());
        let checked = file_check(&root, "shared/project-a/queue");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid shared queue") && error.message.contains("missing directory lease"))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_event_stream_files() {
        let root = unique_test_dir("ctx-events-check");
        let events = root
            .join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session")
            .join("default")
            .join("events.jsonl");
        let parent = events.parent();
        assert!(parent.is_some());
        let Some(parent) = parent else { return };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(
            fs::write(
                &events,
                "{\"type\":\"start\",\"run\":\"r1\",\"response_id\":\"resp_1\"}\n"
            )
            .is_ok()
        );

        let checked = file_check(&root, "home/1000/agent/coder/session/default/events.jsonl");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("provider native field"))
        );

        let model_events = root
            .join("shared")
            .join("project-a")
            .join("model")
            .join("qwen.d")
            .join("session")
            .join("default")
            .join("events.jsonl");
        let parent = model_events.parent();
        assert!(parent.is_some());
        let Some(parent) = parent else { return };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(
            fs::write(
                &model_events,
                "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}\n"
            )
            .is_ok()
        );
        assert!(
            file_check(
                &root,
                "shared/project-a/model/qwen.d/session/default/events.jsonl"
            )
            .is_ok()
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_message_stream_files() {
        let root = unique_test_dir("ctx-messages-check");
        let messages = root
            .join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session")
            .join("default")
            .join("messages.jsonl");
        let parent = messages.parent();
        assert!(parent.is_some());
        let Some(parent) = parent else { return };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(
            fs::write(
                &messages,
                "{\"role\":\"assistant\",\"response_id\":\"resp_1\",\"content\":\"hello\"}\n"
            )
            .is_ok()
        );

        let checked = file_check(
            &root,
            "home/1000/agent/coder/session/default/messages.jsonl",
        );
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("provider native field"))
        );

        assert!(
            fs::write(
                &messages,
                "{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}\n"
            )
            .is_ok()
        );
        assert!(
            file_check(
                &root,
                "home/1000/agent/coder/session/default/messages.jsonl"
            )
            .is_ok()
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_context_jsonl_files() {
        let root = unique_test_dir("ctx-context-jsonl-check");
        let context = root
            .join("shared")
            .join("project-a")
            .join("agent")
            .join("coder")
            .join("session")
            .join("default")
            .join("context");
        assert!(fs::create_dir_all(context.join("swap")).is_ok());
        assert!(
            fs::write(
                context.join("facts.jsonl"),
                "{\"id\":\"f1\",\"text\":\"root is frozen\",\"source\":\"messages:1-2\"}\n"
            )
            .is_ok()
        );
        assert!(
            fs::write(
                context.join("swap").join("index.jsonl"),
                "{\"id\":\"sha256-abc\",\"kind\":\"message_range\",\"source\":\"provider_thread\",\"summary\":\"bad\",\"tokens\":\"10\"}\n"
            )
            .is_ok()
        );

        assert!(
            file_check(
                &root,
                "shared/project-a/agent/coder/session/default/context/facts.jsonl"
            )
            .is_ok()
        );
        let checked = file_check(
            &root,
            "shared/project-a/agent/coder/session/default/context/swap/index.jsonl",
        );
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("invalid context jsonl"))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_check_validates_shared_and_model_session_layouts() {
        let root = unique_test_dir("ctx-shared-model-session-check");
        let shared_agent = root
            .join("shared")
            .join("im-qq-dev")
            .join("agent")
            .join("bot")
            .join("session")
            .join("group-456");
        let model_session = root
            .join("home")
            .join("1000")
            .join("model")
            .join("qwen.d")
            .join("session")
            .join("default");
        create_complete_session_layout(&shared_agent);
        create_complete_session_layout(&model_session);

        assert!(file_check(&root, "shared/im-qq-dev/agent/bot/session/group-456").is_ok());
        assert!(file_check(&root, "home/1000/model/qwen.d/session/default").is_ok());

        assert!(fs::remove_file(model_session.join("messages.jsonl")).is_ok());
        let checked = file_check(&root, "home/1000/model/qwen.d/session/default");
        assert!(
            matches!(checked, Err(ref error) if error.code == 2 && error.message.contains("missing file messages.jsonl"))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn doctor_validates_reference_tree_objects_sessions_and_queue() {
        let root = unique_test_dir("ctx-doctor-reference-tree");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        let ensured = ensure_v1_reference_tree(&root);
        assert!(ensured.is_ok());

        assert!(doctor(&root).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn doctor_reports_reference_tree_layout_breakage() {
        let root = unique_test_dir("ctx-doctor-reference-tree-bad");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        let ensured = ensure_v1_reference_tree(&root);
        assert!(ensured.is_ok());
        assert!(fs::remove_file(root.join("tool").join("fs.read.d").join("schema")).is_ok());
        assert!(
            fs::remove_file(
                root.join("home")
                    .join("1000")
                    .join("agent")
                    .join("coder")
                    .join("session")
                    .join("default")
                    .join("messages.jsonl")
            )
            .is_ok()
        );
        assert!(
            fs::remove_dir_all(
                root.join("shared")
                    .join("project-a")
                    .join("queue")
                    .join("done")
            )
            .is_ok()
        );

        let checked = doctor(&root);
        assert!(matches!(
            checked,
            Err(ref error) if error.code == 69 && error.message.contains("doctor found ABI problems")
        ));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn formats_shared_queue_layout_issues_for_doctor() {
        let formatted = format_shared_queue_layout_issues(&[
            SharedQueueLayoutIssue::MissingDirectory("done".to_owned()),
            SharedQueueLayoutIssue::NotDirectory("failed".to_owned()),
        ]);
        assert_eq!(formatted, "missing directory done, not directory failed");
    }

    #[test]
    fn formats_object_layout_issues_for_file_check() {
        let formatted = format_object_layout_issues(&[
            ObjectLayoutIssue::MissingExecutable("agent/coder".to_owned()),
            ObjectLayoutIssue::InvalidControlValue {
                path: "model/qwen.d/session".to_owned(),
                value: "native_thread".to_owned(),
            },
        ]);
        assert_eq!(
            formatted,
            "missing executable agent/coder, invalid control value model/qwen.d/session=native_thread"
        );
    }

    #[test]
    fn control_file_values_end_in_newline() {
        assert_eq!(newline_terminated("cwd=/work"), "cwd=/work\n");
        assert_eq!(newline_terminated("cwd=/work\n"), "cwd=/work\n");
    }

    #[test]
    fn json_strings_escape_socket_request_values() {
        assert_eq!(json_string("default"), "\"default\"");
        assert_eq!(json_string("quote\"slash\\"), "\"quote\\\"slash\\\\\"");
        assert_eq!(json_string("line\nnext"), "\"line\\nnext\"");
    }

    #[test]
    fn socket_requests_enforce_frame_limit_before_connecting() {
        let request = "x".repeat(MAX_SOCKET_FRAME_BYTES + 1);
        let result = stream_socket_request(Path::new("/does/not/exist.sock"), &request);
        assert!(
            matches!(result, Err(ref error) if error.code == 2 && error.message.contains("EMSGSIZE"))
        );
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!(
            "cortexfs-ctx-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn create_complete_session_layout(session: &Path) {
        let context = session.join("context");
        assert!(fs::create_dir_all(&context).is_ok());
        for file in SESSION_REQUIRED_FILES {
            write_text_file(&session.join(file), session_file_fixture_value(file));
        }
        for file in CONTEXT_REQUIRED_FILES {
            write_text_file(&context.join(file), "ok\n");
        }
        for dir in CONTEXT_REQUIRED_DIRS {
            assert!(fs::create_dir_all(context.join(dir)).is_ok());
        }
        let child = context.join("child").join("rev-1");
        assert!(fs::create_dir_all(child.join("artifact")).is_ok());
        for file in CHILD_RESULT_REQUIRED_FILES {
            write_text_file(&child.join(file), "ok\n");
        }
    }

    fn session_file_fixture_value(file: &str) -> &'static str {
        match file {
            "state" => "idle\n",
            "cwd" => "/work\n",
            "meta.json" => "{\"client\":\"ctx\",\"model\":\"qwen\",\"scope\":\"private\"}\n",
            _ => "ok\n",
        }
    }

    fn write_text_file(path: &Path, content: &str) {
        let Some(parent) = path.parent() else {
            return;
        };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(fs::write(path, content).is_ok());
    }
}
