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
    HelpTopic(String),
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
    Resume {
        agent: String,
        session: Option<String>,
    },
    Send {
        agent: String,
        session: String,
        input: String,
    },
    Agent(AgentArgs),
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
    Tool {
        name: String,
        args: Vec<String>,
    },
    Cat {
        path: String,
    },
    Set {
        path: String,
        value: String,
    },
    Append {
        path: String,
        value: String,
    },
    File(FileArgs),
    ValidateName(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileCommand {
    Info,
    Type,
    Check,
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
}

fn run(args: Vec<OsString>) -> Result<ExitCode, CliError> {
    let cli = parse(args)?;
    match cli.command {
        Command::Help => success(print_help()),
        Command::HelpTopic(topic) => success(print_help_topic(&topic)),
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
        Command::Resume { agent, session } => resume(&cli.root, &agent, session.as_deref()),
        Command::Send {
            agent,
            session,
            input,
        } => send(&cli.root, &agent, &session, &input),
        Command::Agent(args) => agent_command(&cli.root, &args),
        Command::Ping { path } => ping(&cli.root, &path),
        Command::Cancel { path, run } => cancel(&cli.root, &path, &run),
        Command::Doctor => success(doctor(&cli.root)),
        Command::Exec { path, args } => exec_object(&cli.root, &path, &args),
        Command::Tool { name, args } => run_visible_tool(&cli.root, &name, &args),
        Command::Cat { path } => success(file_cat(&cli.root, &path)),
        Command::Set { path, value } => success(file_set(&cli.root, &path, &value)),
        Command::Append { path, value } => success(file_append(&cli.root, &path, &value)),
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

fn required_arg(
    values: &mut impl Iterator<Item = String>,
    message: &str,
) -> Result<String, CliError> {
    values.next().ok_or_else(|| CliError::usage(message))
}

#[expect(clippy::too_many_lines, reason = "flat CLI dispatch keeps subcommand parsing explicit")]
fn parse_command(args: Vec<String>) -> Result<Command, CliError> {
    let mut values = args.into_iter();
    let Some(command) = values.next() else {
        return Ok(Command::Status);
    };
    let rest: Vec<String> = values.collect();
    if is_help_args(&rest) && is_top_level_help_topic(command.as_str()) {
        return Ok(Command::HelpTopic(command));
    }
    if command == "agent" && matches!(rest.as_slice(), [value] if value == "help") {
        return Ok(Command::HelpTopic(command));
    }
    let mut values = rest.into_iter();

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
            let class = required_arg(&mut values, "which requires model, agent, or tool")?;
            let name = required_arg(&mut values, "which requires an object name")?;
            let class = ObjectClass::parse(&class)
                .ok_or_else(|| CliError::usage("which expects model, agent, or tool"))?;
            no_extra_args(values)?;
            Ok(Command::Which(class, name))
        }
        "which-tool" => {
            let name = required_arg(&mut values, "which-tool requires a tool name")?;
            no_extra_args(values)?;
            Ok(Command::Which(ObjectClass::Tool, name))
        }
        "path" => {
            let kind = required_arg(&mut values, "path requires a kind")?;
            match kind.as_str() {
                "shared" => {
                    let name = required_arg(&mut values, "path shared requires a name")?;
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
        "agent" => parse_agent_command(values.collect()),
        "ping" => {
            let path = required_arg(&mut values, "ping requires model/NAME or agent/NAME")?;
            no_extra_args(values)?;
            Ok(Command::Ping { path })
        }
        "cancel" => {
            let path = required_arg(&mut values, "cancel requires model/NAME or agent/NAME")?;
            let run = required_arg(&mut values, "cancel requires a run id")?;
            no_extra_args(values)?;
            Ok(Command::Cancel { path, run })
        }
        "doctor" => {
            no_extra_args(values)?;
            Ok(Command::Doctor)
        }
        "exec" => {
            let path = required_arg(&mut values, "exec requires an ABI object path")?;
            Ok(Command::Exec {
                path,
                args: values.collect(),
            })
        }
        "tool" => {
            let name = required_arg(&mut values, "tool requires a tool name")?;
            Ok(Command::Tool {
                name,
                args: values.collect(),
            })
        }
        "cat" => {
            let path = required_arg(&mut values, "cat requires a path")?;
            no_extra_args(values)?;
            Ok(Command::Cat { path })
        }
        "set" => {
            let path = required_arg(&mut values, "set requires a path")?;
            let value = required_arg(&mut values, "set requires a value")?;
            no_extra_args(values)?;
            Ok(Command::Set { path, value })
        }
        "append" => {
            let path = required_arg(&mut values, "append requires a path")?;
            let value = required_arg(&mut values, "append requires a value")?;
            no_extra_args(values)?;
            Ok(Command::Append { path, value })
        }
        "file" => {
            let args = parse_file_args(values.collect())?;
            Ok(Command::File(args))
        }
        "validate-name" => {
            let name = required_arg(&mut values, "validate-name requires a name")?;
            no_extra_args(values)?;
            Ok(Command::ValidateName(name))
        }
        _ => Err(CliError::usage(format!("unknown command: {command}"))),
    }
}

fn is_help_args(args: &[String]) -> bool {
    matches!(args, [value] if is_help_flag(value))
}

fn is_top_level_help_topic(command: &str) -> bool {
    matches!(
        command,
        "status"
            | "abi"
            | "env"
            | "root"
            | "bootstrap"
            | "mount"
            | "ls"
            | "which"
            | "which-tool"
            | "path"
            | "history"
            | "resume"
            | "send"
            | "agent"
            | "ping"
            | "cancel"
            | "doctor"
            | "exec"
            | "tool"
            | "file"
            | "validate-name"
    )
}

fn is_help_flag(value: &str) -> bool {
    matches!(value, "--help" | "-h")
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
                let next = required_arg(&mut values, "mount --source requires a path")?;
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
    let agent = required_arg(&mut values, &format!("{command} requires an agent name"))?;
    let session = values.next();
    no_extra_args(values)?;
    Ok((agent, session))
}

fn parse_send(
    mut values: impl Iterator<Item = String>,
) -> Result<(String, String, String), CliError> {
    let agent = required_arg(&mut values, "send requires an agent name")?;
    let session = required_arg(&mut values, "send requires a session name")?;
    let input = required_arg(&mut values, "send requires input text")?;
    no_extra_args(values)?;
    Ok((agent, session, input))
}

fn parse_agent_command(args: Vec<String>) -> Result<Command, CliError> {
    let mut values = args.into_iter();
    let command = required_arg(
        &mut values,
        "agent requires new, start, stop, status, ps, send, repl, resume, history, output, pack, tools, children, cancel, watch, or attach",
    )?;
    let rest: Vec<String> = values.collect();
    if is_help_args(&rest) {
        return Ok(Command::HelpTopic(format!("agent {command}")));
    }
    if command == "help" && rest.is_empty() {
        return Ok(Command::HelpTopic("agent".to_owned()));
    }
    let mut values = rest.into_iter();
    match command.as_str() {
        "new" => Ok(Command::Agent(AgentArgs::New(parse_agent_new(values)?))),
        "start" => Ok(Command::Agent(AgentArgs::Start(parse_agent_start(values)?))),
        "stop" => {
            let name = required_arg(&mut values, "agent stop requires an agent name")?;
            no_extra_args(values)?;
            Ok(Command::Agent(AgentArgs::Stop { name }))
        }
        "status" => {
            let name = required_arg(&mut values, "agent status requires an agent name")?;
            no_extra_args(values)?;
            Ok(Command::Agent(AgentArgs::Status { name }))
        }
        "ps" => {
            no_extra_args(values)?;
            Ok(Command::Agent(AgentArgs::Ps))
        }
        "send" => {
            let parsed = parse_agent_send_args(values)?;
            Ok(Command::Agent(AgentArgs::Send {
                name: parsed.name,
                session: parsed.session,
                input: parsed.input,
                raw: parsed.raw,
            }))
        }
        "repl" => {
            let (name, session, raw) = parse_agent_session_raw_args(values, "agent repl")?;
            Ok(Command::Agent(AgentArgs::Repl { name, session, raw }))
        }
        "resume" => {
            let (name, session, raw) = parse_agent_session_raw_args(values, "agent resume")?;
            Ok(Command::Agent(AgentArgs::Resume { name, session, raw }))
        }
        "history" => {
            let (name, session) = parse_agent_session_option_args(values, "agent history")?;
            Ok(Command::Agent(AgentArgs::History { name, session }))
        }
        "output" => {
            let (name, session) = parse_agent_session_option_args(values, "agent output")?;
            Ok(Command::Agent(AgentArgs::Output { name, session }))
        }
        "pack" => {
            let (name, session) = parse_agent_session_option_args(values, "agent pack")?;
            Ok(Command::Agent(AgentArgs::Pack { name, session }))
        }
        "tools" => {
            let name = required_arg(&mut values, "agent tools requires an agent name")?;
            no_extra_args(values)?;
            Ok(Command::Agent(AgentArgs::Tools { name }))
        }
        "children" => {
            let (name, session) = parse_agent_session_option_args(values, "agent children")?;
            Ok(Command::Agent(AgentArgs::Children { name, session }))
        }
        "cancel" => {
            let parsed = parse_agent_cancel_args(values)?;
            Ok(Command::Agent(AgentArgs::Cancel {
                name: parsed.name,
                session: parsed.session,
                run: parsed.run,
                raw: parsed.raw,
            }))
        }
        "watch" => {
            let (name, session) = parse_agent_terminal_args(values, "agent watch")?;
            Ok(Command::Agent(AgentArgs::Watch { name, session }))
        }
        "attach" => {
            let (name, session) = parse_agent_terminal_args(values, "agent attach")?;
            Ok(Command::Agent(AgentArgs::Attach { name, session }))
        }
        _ => Err(CliError::usage(format!("unknown agent command: {command}"))),
    }
}

fn parse_agent_session_option_args(
    mut values: impl Iterator<Item = String>,
    command: &str,
) -> Result<(String, Option<String>), CliError> {
    let name = required_arg(&mut values, &format!("{command} requires an agent name"))?;
    let mut session = None;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" => {
                session = Some(required_arg(
                    &mut values,
                    &format!("{command} --session requires a session name"),
                )?);
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok((name, session))
}

fn parse_agent_session_raw_args(
    mut values: impl Iterator<Item = String>,
    command: &str,
) -> Result<(String, Option<String>, bool), CliError> {
    let name = required_arg(&mut values, &format!("{command} requires an agent name"))?;
    let mut session = None;
    let mut raw = false;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" => {
                session = Some(required_arg(
                    &mut values,
                    &format!("{command} --session requires a session name"),
                )?);
            }
            "--raw" => raw = true,
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok((name, session, raw))
}

struct ParsedAgentSend {
    name: String,
    session: Option<String>,
    raw: bool,
    input: String,
}

struct ParsedAgentCancel {
    name: String,
    session: Option<String>,
    raw: bool,
    run: Option<String>,
}

fn parse_agent_send_args(mut values: impl Iterator<Item = String>) -> Result<ParsedAgentSend, CliError> {
    let name = required_arg(&mut values, "agent send requires an agent name")?;
    let mut session = None;
    let mut raw = false;
    let mut input = Vec::new();
    while let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" if input.is_empty() => {
                session = Some(required_arg(
                    &mut values,
                    "agent send --session requires a session name",
                )?);
            }
            "--raw" if input.is_empty() => raw = true,
            _ => {
                input.push(value);
                input.extend(values);
                break;
            }
        }
    }
    if input.is_empty() {
        return Err(CliError::usage("agent send requires input text"));
    }
    Ok(ParsedAgentSend {
        name,
        session,
        raw,
        input: input.join(" "),
    })
}

fn parse_agent_cancel_args(
    mut values: impl Iterator<Item = String>,
) -> Result<ParsedAgentCancel, CliError> {
    let name = required_arg(&mut values, "agent cancel requires an agent name")?;
    let mut session = None;
    let mut raw = false;
    let mut run = None;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" if run.is_none() => {
                session = Some(required_arg(
                    &mut values,
                    "agent cancel --session requires a session name",
                )?);
            }
            "--raw" if run.is_none() => raw = true,
            _ if run.is_none() => run = Some(value),
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok(ParsedAgentCancel {
        name,
        session,
        raw,
        run,
    })
}

fn parse_agent_start(mut values: impl Iterator<Item = String>) -> Result<AgentStartArgs, CliError> {
    let name = required_arg(&mut values, "agent start requires an agent name")?;
    let mut args = AgentStartArgs {
        name,
        session: "default".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    while let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" => {
                args.session = required_arg(
                    &mut values,
                    "agent start --session requires a session name",
                )?;
            }
            "--cwd" => {
                args.cwd = required_arg(&mut values, "agent start --cwd requires a path")?;
            }
            "--mount" => {
                let source =
                    required_arg(&mut values, "agent start --mount requires a source path")?;
                let target =
                    required_arg(&mut values, "agent start --mount requires a target path")?;
                let mode = required_arg(&mut values, "agent start --mount requires ro or rw")?;
                args.mounts.push(AgentMount {
                    source,
                    target,
                    mode,
                });
            }
            "--no-default-workspace" => {
                args.default_workspace = false;
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok(args)
}

fn parse_agent_terminal_args(
    mut values: impl Iterator<Item = String>,
    command: &str,
) -> Result<(String, Option<String>), CliError> {
    let name = required_arg(&mut values, &format!("{command} requires an agent name"))?;
    let mut session = None;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" => {
                session = Some(required_arg(
                    &mut values,
                    &format!("{command} --session requires a session name"),
                )?);
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok((name, session))
}

fn parse_agent_new(mut values: impl Iterator<Item = String>) -> Result<AgentNewArgs, CliError> {
    let name = required_arg(&mut values, "agent new requires an agent name")?;
    let mut args = AgentNewArgs {
        name,
        temporary: false,
        label: None,
        models: Vec::new(),
        tools: Vec::new(),
        shared: Vec::new(),
        mounts: Vec::new(),
    };

    while let Some(value) = values.next() {
        match value.as_str() {
            "--temp" => {
                args.temporary = true;
            }
            "--label" => {
                let label = required_arg(&mut values, "agent new --label requires a label")?;
                args.label = Some(label);
            }
            "--model" => {
                args.models.push(required_arg(
                    &mut values,
                    "agent new --model requires a model name",
                )?);
            }
            "--tool" => {
                args.tools.push(required_arg(
                    &mut values,
                    "agent new --tool requires a tool name",
                )?);
            }
            "--shared" => {
                let value =
                    required_arg(&mut values, "agent new --shared requires NAME:read|write")?;
                args.shared.push(parse_agent_shared(&value)?);
            }
            "--mount" => {
                let source =
                    required_arg(&mut values, "agent new --mount requires a source path")?;
                let target =
                    required_arg(&mut values, "agent new --mount requires a target path")?;
                let mode = required_arg(&mut values, "agent new --mount requires ro or rw")?;
                args.mounts.push(AgentMount {
                    source,
                    target,
                    mode,
                });
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }

    Ok(args)
}

fn parse_agent_shared(value: &str) -> Result<AgentShared, CliError> {
    let Some((name, access)) = value.split_once(':') else {
        return Err(CliError::usage("agent new --shared expects NAME:read|write"));
    };
    Ok(AgentShared {
        name: name.to_owned(),
        access: access.to_owned(),
    })
}

fn parse_file_args(args: Vec<String>) -> Result<FileArgs, CliError> {
    let mut values = args.into_iter();
    let first = required_arg(&mut values, "file requires a path or subcommand")?;

    let parsed = match first.as_str() {
        "info" => parse_file_path_command(values, FileCommand::Info, "file info requires a path")?,
        "type" => parse_file_path_command(values, FileCommand::Type, "file type requires a path")?,
        "check" => {
            parse_file_path_command(values, FileCommand::Check, "file check requires a path")?
        }
        _ => {
            no_extra_args(values)?;
            FileArgs {
                command: FileCommand::Info,
                path: first,
            }
        }
    };

    Ok(parsed)
}

fn parse_file_path_command(
    mut values: impl Iterator<Item = String>,
    command: FileCommand,
    path_usage: &str,
) -> Result<FileArgs, CliError> {
    let path = required_arg(&mut values, path_usage)?;
    no_extra_args(values)?;
    Ok(FileArgs {
        command,
        path,
    })
}

fn no_extra_args(mut values: impl Iterator<Item = String>) -> Result<(), CliError> {
    values.next().map_or(Ok(()), |value| {
        Err(CliError::usage(format!("unexpected argument: {value}")))
    })
}
