use crate::*;
use cortexfs::object::install::InstallTier;
use cortexfs::object::replace::ReplaceMode;

/// CLI parsing error that exits with a specific code and message.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct CliError {
    pub(crate) code: u8,
    pub(crate) message: String,
}

impl CliError {
    /// Creates an argument-usage error with exit code 2.
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }

    /// Creates an unavailable/internal error with the historical exit code 69.
    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: 69,
            message: message.into(),
        }
    }
}

#[derive(Debug)]
/// Parsed root command context for this invocation.
pub(crate) struct Cli {
    pub(crate) root: PathBuf,
    pub(crate) command: Command,
}

#[derive(Debug)]
/// Top-level parsed command for `ctx`.
pub(crate) enum Command {
    NewSession,
    Help,
    HelpTopic(String),
    Abi,
    Env,
    Root,
    Attach {
        selector: Option<String>,
    },
    Man {
        topic: Option<String>,
    },
    Status,
    Bootstrap {
        source: Option<PathBuf>,
        dry_run: bool,
        check: bool,
    },
    StorageUpdate {
        storage: Option<PathBuf>,
        prune: bool,
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
        agent: Option<String>,
        session: Option<String>,
    },
    Send {
        agent: String,
        session: Option<String>,
        input: String,
    },
    Agent(AgentArgs),
    Terminal(TerminalArgs),
    ObjectInstall {
        source: PathBuf,
        manifest: PathBuf,
        tier: InstallTier,
    },
    ObjectInspect {
        source: PathBuf,
        class: ObjectClass,
        name: String,
        tier: InstallTier,
    },
    ObjectReplace {
        source: PathBuf,
        manifest: PathBuf,
        tier: InstallTier,
        mode: ReplaceMode,
        yes: bool,
    },
    ObjectUninstall {
        source: PathBuf,
        class: ObjectClass,
        name: String,
        tier: InstallTier,
        yes: bool,
    },
    ObjectCheck {
        manifest: PathBuf,
    },
    PackageInstall {
        package: PathBuf,
        source: Option<PathBuf>,
        tier: InstallTier,
    },
    ObjectResidueAudit {
        source: PathBuf,
    },
    ObjectResidueCleanup {
        source: PathBuf,
        path: PathBuf,
        dev: u64,
        ino: u64,
        yes: bool,
    },
    Provider(ProviderArgs),
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
    Schedule(ScheduleArgs),
    ValidateName(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Supported file-related actions under `ctx file`.
pub(crate) enum FileCommand {
    Info,
    Type,
    Check,
}

#[derive(Debug, Eq, PartialEq)]
/// Supported `ctx ls` target.
pub(crate) enum LsTarget {
    Root,
    Path(String),
}

#[derive(Debug)]
/// Arguments parsed from `ctx file`.
pub(crate) struct FileArgs {
    pub(crate) command: FileCommand,
    pub(crate) path: String,
}

#[derive(Debug, Eq, PartialEq)]
/// Parsed schedule arguments for `ctx schedule`.
pub(crate) enum ScheduleArgs {
    Status {
        path: String,
        done: Vec<String>,
    },
    Advance {
        path: String,
        done: Vec<String>,
    },
    Claim {
        path: String,
        child: String,
    },
    Result {
        path: String,
        child: String,
        status: ChildContextStatus,
        result: String,
        refs_jsonl: String,
    },
}

/// Runs the parsed CLI and returns an OS exit code.
pub(crate) fn run(args: Vec<OsString>) -> Result<ExitCode, CliError> {
    let cli = parse(args)?;
    match cli.command {
        Command::NewSession => start_default_session(&cli.root),
        Command::Help => success(print_help()),
        Command::HelpTopic(topic) => success(print_help_topic(&topic)),
        Command::Abi => success(print_abi()),
        Command::Env => success(print_env(&cli.root)),
        Command::Root => success(print_line(&cli.root.display().to_string())),
        Command::Attach { ref selector } => channel_attach(&cli.root, selector.as_deref()),
        Command::Man { topic } => success(print_man(&cli.root, topic.as_deref())),
        Command::Status => success(print_status(&cli.root)),
        Command::Bootstrap {
            source,
            dry_run,
            check,
        } => success(bootstrap_reference_tree(source.as_deref(), dry_run, check)),
        Command::StorageUpdate { storage, prune } => {
            success(update_storage(storage.as_deref(), prune))
        }
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
        Command::Resume { agent, session } => {
            resume_current_session(&cli.root, agent.as_deref(), session.as_deref())
        }
        Command::Send {
            agent,
            session,
            input,
        } => agent_send(
            &cli.root,
            &agent,
            AgentSend {
                session: session.as_deref(),
                input: &input,
                raw: false,
                debug: false,
                approvals: &[],
            },
        ),
        Command::Agent(args) => agent_command(&cli.root, &args),
        Command::Terminal(args) => terminal_action(&cli.root, &args),
        Command::ObjectInstall {
            source,
            manifest,
            tier,
        } => success(install::run_object_install(&source, &manifest, tier)),
        Command::ObjectInspect {
            source,
            class,
            name,
            tier,
        } => success(install::run_object_inspect(&source, class, &name, tier)),
        Command::ObjectReplace {
            source,
            manifest,
            tier,
            mode,
            yes,
        } => success(install::run_object_replace(
            &source, &manifest, tier, mode, yes,
        )),
        Command::ObjectUninstall {
            source,
            class,
            name,
            tier,
            yes,
        } => success(install::run_object_uninstall(
            &source, class, &name, tier, yes,
        )),
        Command::ObjectCheck { manifest } => success(install::run_object_check(&manifest)),
        Command::PackageInstall {
            package,
            source,
            tier,
        } => success(package::run_package_install(
            &package,
            source.as_deref(),
            tier,
        )),
        Command::ObjectResidueAudit { source } => {
            success(residue::run_object_residue_audit(&source))
        }
        Command::ObjectResidueCleanup {
            source,
            path,
            dev,
            ino,
            yes,
        } => success(residue::run_object_residue_cleanup(
            &source, &path, dev, ino, yes,
        )),
        Command::Provider(args) => provider_command(&args),
        Command::Ping { path } => ping(&cli.root, &path),
        Command::Cancel { path, run } => cancel(&cli.root, &path, &run),
        Command::Doctor => success(doctor(&cli.root)),
        Command::Exec { path, args } => exec_object(&cli.root, &path, &args),
        Command::Tool { name, args } => run_visible_tool(&cli.root, &name, &args),
        Command::Cat { path } => success(file_cat(&cli.root, &path)),
        Command::Set { path, value } => success(file_set(&cli.root, &path, &value)),
        Command::Append { path, value } => success(file_append(&cli.root, &path, &value)),
        Command::File(args) => success(file_command(&cli.root, &args)),
        Command::Schedule(args) => success(schedule_command(&cli.root, &args)),
        Command::ValidateName(name) => success(validate_name(&name)),
    }
}

/// Converts a unit-successful command result into `ExitCode::SUCCESS`.
pub(crate) fn success(result: Result<(), CliError>) -> Result<ExitCode, CliError> {
    result.map(|()| ExitCode::SUCCESS)
}

/// Parses command-line arguments into a typed root `Cli` command model.
pub(crate) fn parse(args: Vec<OsString>) -> Result<Cli, CliError> {
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
            "--" => {
                for value in values {
                    rest.push(os_string(value)?);
                }
                break;
            }
            _ => {
                rest.push(text);
                for value in values {
                    rest.push(os_string(value)?);
                }
                break;
            }
        }
    }

    let command = parse_command(rest)?;
    Ok(Cli { root, command })
}

/// Converts a raw `OsString` argument into UTF-8 `String`.
pub(crate) fn os_string(value: OsString) -> Result<String, CliError> {
    value.into_string().map_err(|value| {
        CliError::usage(format!(
            "arguments must be valid UTF-8: {}",
            value.to_string_lossy()
        ))
    })
}

/// Reads the next required CLI argument or returns a usage error.
pub(crate) fn required_arg(
    values: &mut impl Iterator<Item = String>,
    message: &str,
) -> Result<String, CliError> {
    values.next().ok_or_else(|| CliError::usage(message))
}

/// Parses `bootstrap` command arguments and validates allowed flags.
pub(crate) fn parse_bootstrap_command(
    values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let mut source = None;
    let mut dry_run = false;
    let mut check = false;
    for value in values {
        match value.as_str() {
            "--dry-run" => dry_run = true,
            "--check" => check = true,
            _ if source.is_none() && !value.starts_with('-') => {
                source = Some(PathBuf::from(value));
            }
            _ => {
                return Err(CliError::usage(format!(
                    "unexpected argument: {value} (expected [--check] [--dry-run] [SOURCE])"
                )));
            }
        }
    }
    if dry_run && check {
        return Err(CliError::usage(
            "bootstrap accepts only one of --check or --dry-run",
        ));
    }
    Ok(Command::Bootstrap {
        source,
        dry_run,
        check,
    })
}

/// Parses `storage` command arguments and validates the `update` action.
pub(crate) fn parse_storage_command(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    if required_arg(&mut values, "storage requires update")? != "update" {
        return Err(CliError::usage("storage supports only update"));
    }
    let mut storage = None;
    let mut prune = false;
    for value in values {
        match value.as_str() {
            "--prune" if !prune => prune = true,
            _ if storage.is_none() && !value.starts_with('-') => {
                storage = Some(PathBuf::from(value));
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok(Command::StorageUpdate { storage, prune })
}

/// Parses all top-level commands by dispatching on command name and arguments.
#[expect(
    clippy::too_many_lines,
    reason = "flat CLI dispatch keeps subcommand parsing explicit"
)]
pub(crate) fn parse_command(args: Vec<String>) -> Result<Command, CliError> {
    let mut values = args.into_iter();
    let Some(command) = values.next() else {
        return Ok(Command::NewSession);
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
        "help" => {
            let topic = values.next();
            no_extra_args(values)?;
            topic.map_or(Ok(Command::Help), |topic| Ok(Command::HelpTopic(topic)))
        }
        "--help" | "-h" => {
            no_extra_args(values)?;
            Ok(Command::Help)
        }
        "abi" => {
            no_extra_args(values)?;
            Ok(Command::Abi)
        }
        "env" => {
            no_extra_args(values)?;
            Ok(Command::Env)
        }
        "root" => {
            no_extra_args(values)?;
            Ok(Command::Root)
        }
        "attach" => {
            let selector = values.next();
            no_extra_args(values)?;
            Ok(Command::Attach { selector })
        }
        "man" => {
            let topic = values.next();
            no_extra_args(values)?;
            Ok(Command::Man { topic })
        }
        "status" => {
            no_extra_args(values)?;
            Ok(Command::Status)
        }
        "bootstrap" => parse_bootstrap_command(values),
        "storage" => parse_storage_command(values),
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
            let (agent, session) = parse_resume(values)?;
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
        "inspect" => {
            let (target, session) = parse_agent_session_option_args(values, "inspect")?;
            let target = target
                .strip_prefix(&format!("{CTX_ROOT}/"))
                .unwrap_or(&target);
            let name = target
                .strip_prefix("agent/")
                .filter(|name| is_object_name(name))
                .ok_or_else(|| CliError::usage("inspect expects agent/NAME"))?
                .to_owned();
            Ok(Command::Agent(AgentArgs::Inspect { name, session }))
        }
        "agent" => parse_agent_command(values.collect()),
        "terminal" => parse_terminal_command(values.collect()),
        "object" => install::parse_object_command(values),
        "install" => package::parse_package_install_command(values),
        "provider" => parse_provider_command(values.collect()),
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
        "schedule" => {
            let args = parse_schedule_args(values.collect())?;
            Ok(Command::Schedule(args))
        }
        "validate-name" => {
            let name = required_arg(&mut values, "validate-name requires a name")?;
            no_extra_args(values)?;
            Ok(Command::ValidateName(name))
        }
        _ => Err(CliError::usage(format!("unknown command: {command}"))),
    }
}

/// Returns true when args contain only a single CLI help flag.
pub(crate) fn is_help_args(args: &[String]) -> bool {
    matches!(args, [value] if is_help_flag(value))
}

/// Returns true when a command name supports `help <command>` usage.
pub(crate) fn is_top_level_help_topic(command: &str) -> bool {
    matches!(
        command,
        "status"
            | "abi"
            | "env"
            | "root"
            | "attach"
            | "man"
            | "bootstrap"
            | "storage"
            | "mount"
            | "ls"
            | "which"
            | "which-tool"
            | "path"
            | "history"
            | "resume"
            | "send"
            | "inspect"
            | "agent"
            | "terminal"
            | "object"
            | "install"
            | "provider"
            | "ping"
            | "cancel"
            | "doctor"
            | "exec"
            | "tool"
            | "file"
            | "schedule"
            | "validate-name"
    )
}
