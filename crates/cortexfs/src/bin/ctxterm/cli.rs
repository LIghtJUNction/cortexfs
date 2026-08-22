use crate::*;

pub(crate) fn parse_args(args: Vec<OsString>) -> Result<CtxtermCommand, CtxtermError> {
    let mut values = args.into_iter();
    let Some(first) = values.next() else {
        return Err(CtxtermError::usage(
            "--broker AGENT SESSION UNIT is required",
        ));
    };
    if first == "--help" || first == "-h" {
        return Ok(CtxtermCommand::Help);
    }
    if first != "--broker" {
        return Err(CtxtermError::usage(
            "--broker AGENT SESSION UNIT is required",
        ));
    }
    let broker = BrokerConfig {
        agent: next_text(&mut values)?,
        session: next_text(&mut values)?,
        unit: next_text(&mut values)?,
    };
    let next = values.next();
    let program = match next.as_deref() {
        None => OsString::from(DEFAULT_SHELL),
        Some(value) if value == "--" => values
            .next()
            .ok_or_else(|| CtxtermError::usage("-- requires a command"))?,
        Some(_value) => next.ok_or_else(|| CtxtermError::usage("missing command"))?,
    };
    Ok(CtxtermCommand::Run {
        broker,
        program,
        args: values.collect(),
    })
}

fn next_text(values: &mut impl Iterator<Item = OsString>) -> Result<String, CtxtermError> {
    values
        .next()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CtxtermError::usage("--broker requires AGENT SESSION UNIT"))
}

pub(crate) fn print_help() -> Result<(), CtxtermError> {
    write_stdout(
        "\
ctxterm - CortexFS agent terminal supervisor

usage:
  ctxterm --broker AGENT SESSION UNIT [-- COMMAND [ARG...]]
",
    )
}
