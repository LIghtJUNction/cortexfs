use crate::*;

#[cfg(test)]
mod tests;

/// Parsed terminal-resource command.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum TerminalArgs {
    Create {
        agent: String,
        session: String,
        cwd: String,
    },
    List,
    Status {
        id: String,
    },
    Watch {
        id: String,
    },
    Attach {
        id: String,
    },
}

/// Parses the canonical terminal-resource command family.
pub(crate) fn parse_terminal_command(args: Vec<String>) -> Result<Command, CliError> {
    let mut values = args.into_iter();
    let command = required_arg(
        &mut values,
        "terminal requires create, list, status, watch, or attach",
    )?;
    let rest = values.collect::<Vec<_>>();
    if is_help_args(&rest) {
        return Ok(Command::HelpTopic(format!("terminal {command}")));
    }
    if command == "help" && rest.is_empty() {
        return Ok(Command::HelpTopic("terminal".to_owned()));
    }
    let values = rest.into_iter();
    match command.as_str() {
        "create" => parse_terminal_create(values),
        "list" => {
            no_extra_args(values)?;
            Ok(Command::Terminal(TerminalArgs::List))
        }
        "status" => parse_terminal_id(values, |id| TerminalArgs::Status { id }),
        "watch" => parse_terminal_id(values, |id| TerminalArgs::Watch { id }),
        "attach" => parse_terminal_id(values, |id| TerminalArgs::Attach { id }),
        _ => Err(CliError::usage(format!(
            "unknown terminal command: {command}"
        ))),
    }
}

fn parse_terminal_create(mut values: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let agent = required_arg(&mut values, "terminal create requires an agent name")?;
    let mut session = "default".to_owned();
    let mut cwd = "/workspace".to_owned();
    while let Some(flag) = values.next() {
        let target = match flag.as_str() {
            "--session" | "-s" => &mut session,
            "--cwd" => &mut cwd,
            _ => return Err(CliError::usage(format!("unexpected argument: {flag}"))),
        };
        *target = required_arg(
            &mut values,
            &format!("terminal create {flag} requires a value"),
        )?;
    }
    Ok(Command::Terminal(TerminalArgs::Create {
        agent,
        session,
        cwd,
    }))
}

fn parse_terminal_id(
    mut values: impl Iterator<Item = String>,
    build: impl FnOnce(String) -> TerminalArgs,
) -> Result<Command, CliError> {
    let id = required_arg(&mut values, "terminal command requires a terminal id")?;
    no_extra_args(values)?;
    Ok(Command::Terminal(build(id)))
}
