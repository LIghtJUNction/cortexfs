use crate::*;

pub(crate) fn parse_agent_session(
    mut values: impl Iterator<Item = String>,
    command: &str,
) -> Result<(String, Option<String>), CliError> {
    let agent = required_arg(&mut values, &format!("{command} requires an agent name"))?;
    let mut session = None;
    if let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" => {
                session = Some(required_arg(
                    &mut values,
                    &format!("{command} --session requires a session name"),
                )?);
            }
            _ => session = Some(value),
        }
    }
    no_extra_args(values)?;
    Ok((agent, session))
}

pub(crate) fn parse_send(
    mut values: impl Iterator<Item = String>,
) -> Result<(String, Option<String>, String), CliError> {
    let agent = required_arg(&mut values, "send requires an agent name")?;
    let mut rest: Vec<String> = values.collect();
    if rest.is_empty() {
        return Err(CliError::usage("send requires input text"));
    }
    if matches!(rest.first().map(String::as_str), Some("--session" | "-s")) {
        if rest.len() < 3 {
            return Err(CliError::usage(
                "send --session requires a session and input text",
            ));
        }
        let session = rest.remove(1);
        rest.remove(0);
        return Ok((agent, Some(session), rest.join(" ")));
    }
    if rest.len() == 1 {
        return Ok((agent, None, rest.remove(0)));
    }
    let session = rest.remove(0);
    Ok((agent, Some(session), rest.join(" ")))
}

#[expect(
    clippy::too_many_lines,
    reason = "flat agent subcommand dispatch keeps accepted flags auditable"
)]
pub(crate) fn parse_agent_command(args: Vec<String>) -> Result<Command, CliError> {
    let mut values = args.into_iter();
    let command = required_arg(
        &mut values,
        "agent requires new, apply, start, stop, status, env, ps, send, chat, repl, resume, history, output, pack, trajectory, session, prompt, tools, children, wait, cancel, watch, or attach",
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
        "apply" => Ok(Command::Agent(parse_agent_apply(values)?)),
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
        "env" => {
            let name = required_arg(&mut values, "agent env requires an agent name")?;
            no_extra_args(values)?;
            Ok(Command::Agent(AgentArgs::Env { name }))
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
                approvals: parsed.approvals,
            }))
        }
        "chat" | "repl" => {
            let command = format!("agent {command}");
            let parsed = parse_agent_session_raw_args(values, &command, true)?;
            Ok(Command::Agent(AgentArgs::Repl {
                name: parsed.name,
                session: parsed.session,
                raw: parsed.raw,
                approvals: parsed.approvals,
            }))
        }
        "resume" => {
            let parsed = parse_agent_session_raw_args(values, "agent resume", false)?;
            Ok(Command::Agent(AgentArgs::Resume {
                name: parsed.name,
                session: parsed.session,
                raw: parsed.raw,
            }))
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
        "trajectory" => {
            let (name, session) = parse_agent_session_option_args(values, "agent trajectory")?;
            Ok(Command::Agent(AgentArgs::Trajectory { name, session }))
        }
        "session" => parse_agent_session_admin(values),
        "prompt" => {
            let name = required_arg(&mut values, "agent prompt requires an agent name")?;
            no_extra_args(values)?;
            Ok(Command::Agent(AgentArgs::Prompt { name }))
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
        "wait" => {
            let (name, session, child) = parse_agent_wait_args(values)?;
            Ok(Command::Agent(AgentArgs::Wait {
                name,
                session,
                child,
            }))
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
            let (name, session) = parse_agent_session_option_args(values, "agent watch")?;
            Ok(Command::Agent(AgentArgs::Watch { name, session }))
        }
        "attach" => {
            let (name, session) = parse_agent_session_option_args(values, "agent attach")?;
            Ok(Command::Agent(AgentArgs::Attach { name, session }))
        }
        _ => Err(CliError::usage(format!("unknown agent command: {command}"))),
    }
}

pub(crate) fn parse_agent_session_admin(
    mut values: impl Iterator<Item = String>,
) -> Result<Command, CliError> {
    let command = required_arg(&mut values, "agent session requires gc")?;
    match command.as_str() {
        "gc" => Ok(Command::Agent(AgentArgs::SessionGc(
            parse_agent_session_gc(values)?,
        ))),
        _ => Err(CliError::usage(format!(
            "unknown agent session command: {command}"
        ))),
    }
}

pub(crate) fn parse_agent_session_gc(
    mut values: impl Iterator<Item = String>,
) -> Result<AgentSessionGcArgs, CliError> {
    let name = required_arg(&mut values, "agent session gc requires an agent name")?;
    let mut args = AgentSessionGcArgs {
        name,
        dry_run: true,
        yes: false,
        keep: Vec::new(),
        patterns: Vec::new(),
        older_than_days: None,
    };
    while let Some(value) = values.next() {
        match value.as_str() {
            "--dry-run" => args.dry_run = true,
            "--yes" => {
                args.yes = true;
                args.dry_run = false;
            }
            "--keep" => args.keep.push(required_arg(
                &mut values,
                "agent session gc --keep requires a session name",
            )?),
            "--match" => args.patterns.push(required_arg(
                &mut values,
                "agent session gc --match requires a glob pattern",
            )?),
            "--older-than-days" => {
                let days = required_arg(
                    &mut values,
                    "agent session gc --older-than-days requires a number",
                )?;
                args.older_than_days = Some(
                    days.parse()
                        .map_err(|_error| CliError::usage("invalid --older-than-days value"))?,
                );
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok(args)
}

pub(crate) fn parse_agent_session_option_args(
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

pub(crate) fn parse_agent_wait_args(
    mut values: impl Iterator<Item = String>,
) -> Result<(String, Option<String>, String), CliError> {
    let name = required_arg(&mut values, "agent wait requires an agent name")?;
    let mut session = None;
    let mut child = None;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" => {
                session = Some(required_arg(
                    &mut values,
                    "agent wait --session requires a session name",
                )?);
            }
            _ if child.is_none() => child = Some(value),
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    let child = child.ok_or_else(|| CliError::usage("agent wait requires a child name"))?;
    Ok((name, session, child))
}

pub(crate) fn parse_agent_session_raw_args(
    mut values: impl Iterator<Item = String>,
    command: &str,
    allow_approval: bool,
) -> Result<ParsedAgentSessionRaw, CliError> {
    let name = required_arg(&mut values, &format!("{command} requires an agent name"))?;
    let mut session = None;
    let mut raw = false;
    let mut approvals = Vec::new();
    while let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" => {
                session = Some(required_arg(
                    &mut values,
                    &format!("{command} --session requires a session name"),
                )?);
            }
            "--raw" => raw = true,
            "--approve" if allow_approval => {
                let approval = required_arg(
                    &mut values,
                    &format!("{command} --approve requires a tool name"),
                )?;
                require_cli_name("approved tool name", &approval)?;
                approvals.push(approval);
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok(ParsedAgentSessionRaw {
        name,
        session,
        raw,
        approvals,
    })
}

pub(crate) struct ParsedAgentSessionRaw {
    pub(crate) name: String,
    pub(crate) session: Option<String>,
    pub(crate) raw: bool,
    pub(crate) approvals: Vec<String>,
}

pub(crate) struct ParsedAgentSend {
    pub(crate) name: String,
    pub(crate) session: Option<String>,
    pub(crate) raw: bool,
    pub(crate) input: String,
    pub(crate) approvals: Vec<String>,
}

pub(crate) struct ParsedAgentCancel {
    pub(crate) name: String,
    pub(crate) session: Option<String>,
    pub(crate) raw: bool,
    pub(crate) run: Option<String>,
}

pub(crate) fn parse_agent_send_args(
    mut values: impl Iterator<Item = String>,
) -> Result<ParsedAgentSend, CliError> {
    let name = required_arg(&mut values, "agent send requires an agent name")?;
    let mut session = None;
    let mut raw = false;
    let mut input = Vec::new();
    let mut approvals = Vec::new();
    while let Some(value) = values.next() {
        match value.as_str() {
            "--session" | "-s" if input.is_empty() => {
                session = Some(required_arg(
                    &mut values,
                    "agent send --session requires a session name",
                )?);
            }
            "--raw" if input.is_empty() => raw = true,
            "--approve" if input.is_empty() => {
                let approval =
                    required_arg(&mut values, "agent send --approve requires a tool name")?;
                require_cli_name("approved tool name", &approval)?;
                approvals.push(approval);
            }
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
        approvals,
    })
}

pub(crate) fn parse_agent_cancel_args(
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

pub(crate) fn parse_agent_start(
    mut values: impl Iterator<Item = String>,
) -> Result<AgentStartArgs, CliError> {
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
                args.session =
                    required_arg(&mut values, "agent start --session requires a session name")?;
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

pub(crate) fn parse_agent_new(
    mut values: impl Iterator<Item = String>,
) -> Result<AgentNewArgs, CliError> {
    let mut name = None;
    let mut from = None;
    let mut args = AgentNewArgs {
        name: String::new(),
        temporary: false,
        parent: None,
        label: None,
        models: Vec::new(),
        tools: Vec::new(),
        shared: Vec::new(),
        mounts: Vec::new(),
        instructions: None,
        description: None,
    };

    while let Some(value) = values.next() {
        match value.as_str() {
            "--from" => {
                from = Some(required_arg(
                    &mut values,
                    "agent new --from requires agent.yaml path, directory, or short name",
                )?);
            }
            "--temp" => {
                args.temporary = true;
            }
            "--parent" => {
                args.parent = Some(required_arg(
                    &mut values,
                    "agent new --parent requires an agent parent reference",
                )?);
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
                let source = required_arg(&mut values, "agent new --mount requires a source path")?;
                let target = required_arg(&mut values, "agent new --mount requires a target path")?;
                let mode = required_arg(&mut values, "agent new --mount requires ro or rw")?;
                args.mounts.push(AgentMount {
                    source,
                    target,
                    mode,
                });
            }
            _ if name.is_none() && !value.starts_with('-') => {
                name = Some(value);
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }

    args.name = name.unwrap_or_default();
    if let Some(path) = from {
        let profile = load_agent_profile(Path::new(&path))?;
        args = agent_new_args_from_profile(profile, args)?;
    } else if args.name.is_empty() {
        return Err(CliError::usage(
            "agent new requires an agent name or --from agent.yaml",
        ));
    }

    Ok(args)
}

pub(crate) fn parse_agent_apply(
    mut values: impl Iterator<Item = String>,
) -> Result<AgentArgs, CliError> {
    let name = required_arg(&mut values, "agent apply requires an agent name")?;
    let mut from = None;
    while let Some(value) = values.next() {
        match value.as_str() {
            "--from" => {
                from = Some(required_arg(
                    &mut values,
                    "agent apply --from requires agent.yaml path, directory, or short name",
                )?);
            }
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    let from = from.ok_or_else(|| CliError::usage("agent apply requires --from agent.yaml"))?;
    Ok(AgentArgs::Apply { name, from })
}

pub(crate) fn parse_agent_shared(value: &str) -> Result<AgentShared, CliError> {
    let Some((name, access)) = value.split_once(':') else {
        return Err(CliError::usage(
            "agent new --shared expects NAME:read|write",
        ));
    };
    Ok(AgentShared {
        name: name.to_owned(),
        access: access.to_owned(),
    })
}

#[cfg(test)]
mod approval_tests {
    use super::*;

    #[test]
    fn send_and_repl_parse_repeatable_explicit_approvals() {
        let send = parse_agent_send_args(
            [
                "coder",
                "--approve",
                "example.echo",
                "--approve",
                "fs.read",
                "go",
            ]
            .into_iter()
            .map(str::to_owned),
        );
        assert!(send.is_ok());
        let Ok(send) = send else {
            return;
        };
        assert_eq!(send.approvals, ["example.echo", "fs.read"]);
        assert_eq!(send.input, "go");
        let parsed = parse_agent_session_raw_args(
            ["coder", "--approve", "example.echo"]
                .into_iter()
                .map(str::to_owned),
            "agent repl",
            true,
        );
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else {
            return;
        };
        assert!(!parsed.raw);
        assert_eq!(parsed.approvals, ["example.echo"]);
    }
}
