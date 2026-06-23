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
    AbiPathKind, AgentControlIssue, CTX_ROOT, ContextJsonlIssue, ContextPackIssue,
    EventStreamIssue, MAX_SOCKET_FRAME_BYTES, MessageStreamIssue, ModelCapabilityIssue,
    ModelDriverRouteError, MountTable, ObjectClass, ObjectLayoutIssue, PolicyV0, ROOT_ENTRIES,
    SessionControlIssue, SessionIndexIssue, SessionIndexKind, SessionLayoutIssue,
    SharedQueueLayoutIssue, ToolPath, ToolSchemaIssue, classify_abi_path, ensure_v1_reference_tree,
    inspect_agent_control, inspect_context_jsonl, inspect_context_pack_json,
    inspect_event_stream_jsonl, inspect_message_stream_jsonl, inspect_model_capabilities,
    inspect_object_layout, inspect_session_control, inspect_session_index, inspect_session_layout,
    inspect_shared_queue_layout, inspect_tool_schema_json, is_executable_file, is_model_name,
    is_object_name, parse_abi_path, parse_model_driver_routes,
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
    value.into_string().map_err(|value| {
        CliError::usage(format!(
            "arguments must be valid UTF-8: {}",
            value.to_string_lossy()
        ))
    })
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
    match class {
        ObjectClass::Model if !is_model_name(name) => {
            return Err(CliError::usage(format!("invalid model name: {name}")));
        }
        ObjectClass::Agent if !is_object_name(name) => {
            return Err(CliError::usage(format!("invalid object name: {name}")));
        }
        ObjectClass::Tool => return which_tool(root, name),
        _ => {}
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
        let names = object_names_for_doctor(&dir, class)?;
        for name in names {
            ok &= doctor_object(root, class, &name)?;
        }
    }
    Ok(ok)
}

fn object_names_for_doctor(dir: &Path, class: ObjectClass) -> Result<Vec<String>, CliError> {
    if class != ObjectClass::Model {
        return read_dir_names(dir).map(|names| {
            names
                .into_iter()
                .filter(|name| is_object_name(name))
                .collect()
        });
    }

    let mut names = Vec::new();
    for provider in read_dir_names(dir)? {
        if !is_object_name(&provider) {
            continue;
        }
        let provider_dir = dir.join(&provider);
        if !provider_dir.is_dir() {
            continue;
        }
        for entry in read_dir_names(&provider_dir)? {
            let model = entry
                .strip_suffix(".d")
                .or_else(|| entry.strip_suffix(".sock"))
                .unwrap_or(&entry);
            let name = format!("{provider}/{model}");
            if is_model_name(&name) && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    Ok(names)
}

fn doctor_object(root: &Path, class: ObjectClass, name: &str) -> Result<bool, CliError> {
    let report = inspect_object_layout(root, class, name);
    if report.is_ok() {
        print_line(&format!("ok {}/{}", class.as_str(), name))?;
        Ok(true)
    } else {
        print_line(&format!(
            "invalid {}/{}: {}",
            class.as_str(),
            name,
            format_object_layout_issues(report.issues())
        ))?;
        Ok(false)
    }
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
    for provider in read_dir_names(model_root)? {
        if !is_object_name(&provider) {
            continue;
        }
        let provider_root = model_root.join(&provider);
        if !provider_root.is_dir() {
            continue;
        }
        for model_dir in read_dir_names(&provider_root)? {
            let Some(model) = model_dir.strip_suffix(".d") else {
                continue;
            };
            let model_name = format!("{provider}/{model}");
            if !is_model_name(&model_name) {
                continue;
            }
            collect_named_sessions(
                &provider_root.join(&model_dir).join("session"),
                &format!("{label_prefix}/{provider}/{model_dir}/session"),
                sessions,
            )?;
        }
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
    let parsed = parse_abi_path(&abi_path);
    let shape = parsed.stable_type();
    if shape == "ctx.unknown" {
        return file_classify(root, path);
    }

    if file_check_policy_or_mount(parsed, &resolved)? {
        return Ok(());
    }

    if parsed.is_tool_schema() {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_tool_schema_json(&content);
        return check_report("tool schema", report.is_ok(), || {
            format_tool_schema_issues(report.issues())
        });
    }

    if matches!(parsed, AbiPathKind::SharedQueueRoot { .. }) {
        let report = inspect_shared_queue_layout(&resolved);
        return check_report("shared queue", report.is_ok(), || {
            format_shared_queue_layout_issues(report.issues())
        });
    }

    if parsed.model_control_file() == Some("cap") {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_model_capabilities(&content);
        return check_report("model capabilities", report.is_ok(), || {
            format_model_capability_issues(report.issues())
        });
    }

    if file_check_model_driver(parsed, &resolved)? {
        return Ok(());
    }

    if let Some(kind) = parsed.agent_control_kind() {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_agent_control(kind, &content);
        return check_report("agent control", report.is_ok(), || {
            format_agent_control_issues(report.issues())
        });
    }

    if let Some(kind) = parsed.session_index_kind() {
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
        return check_report("session index", report.is_ok(), || {
            format_session_index_issues(report.issues())
        });
    }

    if let Some(kind) = parsed.session_control_kind() {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_session_control(kind, &content);
        return check_report("session control", report.is_ok(), || {
            format_session_control_issues(report.issues())
        });
    }

    if file_check_jsonl_content(parsed, &resolved)? {
        return Ok(());
    }

    if parsed.is_context_pack() {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_context_pack_json(&content);
        return check_report("context pack", report.is_ok(), || {
            format_context_pack_issues(report.issues())
        });
    }

    if shape == "ctx.session.dir" && parsed.is_session_instance() {
        let report = inspect_session_layout(&resolved);
        return check_report("session layout", report.is_ok(), || {
            format_session_layout_issues(report.issues())
        });
    }

    if let Some((class, name)) = parsed.executable_object() {
        let report = inspect_object_layout(root, class, &name);
        return check_report("object layout", report.is_ok(), || {
            format_object_layout_issues(report.issues())
        });
    }

    print_line(shape)
}

fn check_report(
    label: &str,
    is_ok: bool,
    format_issues: impl FnOnce() -> String,
) -> Result<(), CliError> {
    if is_ok {
        return print_line("ok");
    }
    Err(CliError::usage(format!(
        "invalid {label}: {}",
        format_issues()
    )))
}

fn file_check_policy_or_mount(parsed: AbiPathKind<'_>, resolved: &Path) -> Result<bool, CliError> {
    if parsed.control_file() == Some("policy") {
        let content = read_file_to_string(resolved)?;
        PolicyV0::parse(&content)
            .map_err(|error| CliError::usage(format!("invalid policy: {error:?}")))?;
        print_line("ok")?;
        return Ok(true);
    }

    if parsed.control_file() == Some("mount") {
        let content = read_file_to_string(resolved)?;
        MountTable::parse(&content)
            .map_err(|error| CliError::usage(format!("invalid mount: {error:?}")))?;
        print_line("ok")?;
        return Ok(true);
    }

    Ok(false)
}

fn file_check_jsonl_content(parsed: AbiPathKind<'_>, resolved: &Path) -> Result<bool, CliError> {
    if matches!(
        parsed,
        AbiPathKind::SessionFile {
            file: "messages.jsonl",
            ..
        }
    ) {
        let content = read_file_to_string(resolved)?;
        let report = inspect_message_stream_jsonl(&content);
        return check_report("message stream", report.is_ok(), || {
            format_message_stream_issues(report.issues())
        })
        .map(|()| true);
    }

    if matches!(
        parsed,
        AbiPathKind::SessionFile {
            file: "events.jsonl",
            ..
        }
    ) {
        let content = read_file_to_string(resolved)?;
        let report = inspect_event_stream_jsonl(&content);
        return check_report("event stream", report.is_ok(), || {
            format_event_stream_issues(report.issues())
        })
        .map(|()| true);
    }

    if let Some(kind) = parsed.context_jsonl_kind() {
        let content = read_file_to_string(resolved)?;
        let report = inspect_context_jsonl(kind, &content);
        return check_report("context jsonl", report.is_ok(), || {
            format_context_jsonl_issues(report.issues())
        })
        .map(|()| true);
    }

    Ok(false)
}

fn file_check_model_driver(parsed: AbiPathKind<'_>, resolved: &Path) -> Result<bool, CliError> {
    if parsed.model_control_file() != Some("driver") {
        return Ok(false);
    }

    let content = read_file_to_string(resolved)?;
    match parse_model_driver_routes(&content) {
        Ok(_) => {
            print_line("ok")?;
            Ok(true)
        }
        Err(error) => Err(CliError::usage(format!(
            "invalid model driver routes: {}",
            format_model_driver_route_error(&error)
        ))),
    }
}

#[cfg(test)]
fn is_model_capability_path(path: &str) -> bool {
    parse_abi_path(path).model_control_file() == Some("cap")
}

#[cfg(test)]
fn is_model_driver_path(path: &str) -> bool {
    parse_abi_path(path).model_control_file() == Some("driver")
}

#[cfg(test)]
fn is_tool_schema_path(path: &str) -> bool {
    matches!(
        parse_abi_path(path),
        AbiPathKind::ObjectControl {
            class: ObjectClass::Tool,
            file: "schema",
            ..
        }
    )
}

#[cfg(test)]
fn is_shared_tool_schema_path(path: &str) -> bool {
    matches!(
        parse_abi_path(path),
        AbiPathKind::SharedToolControl { file: "schema", .. }
    )
}

#[cfg(test)]
fn is_shared_queue_root_path(path: &str) -> bool {
    matches!(parse_abi_path(path), AbiPathKind::SharedQueueRoot { .. })
}

#[cfg(test)]
fn agent_control_path_kind(path: &str) -> Option<cortexfs::AgentControlKind> {
    parse_abi_path(path).agent_control_kind()
}

#[cfg(test)]
fn is_durable_session_instance_path(path: &str) -> bool {
    parse_abi_path(path).is_session_instance()
}

#[cfg(test)]
fn session_index_path_kind(path: &str) -> Option<SessionIndexKind> {
    parse_abi_path(path).session_index_kind()
}

#[cfg(test)]
fn is_session_events_path(path: &str) -> bool {
    matches!(
        parse_abi_path(path),
        AbiPathKind::SessionFile {
            file: "events.jsonl",
            ..
        }
    )
}

#[cfg(test)]
fn is_session_messages_path(path: &str) -> bool {
    matches!(
        parse_abi_path(path),
        AbiPathKind::SessionFile {
            file: "messages.jsonl",
            ..
        }
    )
}

#[cfg(test)]
fn session_control_path_kind(path: &str) -> Option<cortexfs::SessionControlKind> {
    parse_abi_path(path).session_control_kind()
}

#[cfg(test)]
fn is_context_pack_path(path: &str) -> bool {
    parse_abi_path(path).is_context_pack()
}

#[cfg(test)]
fn context_jsonl_path_kind(path: &str) -> Option<cortexfs::ContextJsonlKind> {
    parse_abi_path(path).context_jsonl_kind()
}

#[cfg(test)]
fn executable_object_path(path: &str) -> Option<(ObjectClass, String)> {
    parse_abi_path(path)
        .executable_object()
        .map(|(class, name)| (class, name.into_owned()))
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
        output.push_str(issue.kind());
        output.push(' ');
        output.push_str(issue.path());
        if let Some(value) = issue.value() {
            output.push('=');
            output.push_str(value);
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
        output.push_str(issue.kind());
        output.push(' ');
        output.push_str(issue.path());
        if let Some(value) = issue.value() {
            output.push('=');
            output.push_str(value);
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

fn format_model_driver_route_error(error: &ModelDriverRouteError) -> String {
    match *error {
        ModelDriverRouteError::Empty => "empty driver route table".to_owned(),
        ModelDriverRouteError::MissingEquals { line } => {
            format!("missing equals line {line}")
        }
        ModelDriverRouteError::UnknownUseCase { line, ref value } => {
            format!("unknown driver use case line {line} {value}")
        }
        ModelDriverRouteError::DuplicateUseCase { line, ref value } => {
            format!("duplicate driver use case line {line} {value}")
        }
        ModelDriverRouteError::EmptyDriver { line } => {
            format!("empty driver line {line}")
        }
        ModelDriverRouteError::InvalidDriverName { line, ref value } => {
            format!("invalid driver name line {line} {value}")
        }
    }
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
            ToolSchemaIssue::InvalidSchema => write!(output, "invalid schema"),
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
            .map_err(|error| {
                CliError::usage(format!(
                    "absolute file path must be under CTX_ROOT: {error}"
                ))
            });
    }
    Ok(path.to_owned())
}

fn resolve_abi_path(root: &Path, path: &str) -> Result<PathBuf, CliError> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return candidate
            .strip_prefix(root)
            .map(|relative| root.join(relative))
            .map_err(|error| {
                CliError::usage(format!(
                    "absolute file path must be under CTX_ROOT: {error}"
                ))
            });
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
                push_json_unicode_escape(&mut escaped, character);
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn push_json_unicode_escape(output: &mut String, character: char) {
    let value = u32::from(character);
    output.push_str("\\u");
    output.push(hex_digit((value >> 12) & 0x0f));
    output.push(hex_digit((value >> 8) & 0x0f));
    output.push(hex_digit((value >> 4) & 0x0f));
    output.push(hex_digit(value & 0x0f));
}

fn hex_digit(value: u32) -> char {
    match value {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'a',
        11 => 'b',
        12 => 'c',
        13 => 'd',
        14 => 'e',
        15 => 'f',
        _ => '?',
    }
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
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/ctx_tests.rs"
    ));
}
