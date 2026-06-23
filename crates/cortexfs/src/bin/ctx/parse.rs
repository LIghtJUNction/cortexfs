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
