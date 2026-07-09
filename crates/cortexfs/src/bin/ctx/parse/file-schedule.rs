use crate::*;

pub(crate) fn parse_file_args(args: Vec<String>) -> Result<FileArgs, CliError> {
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

pub(crate) fn parse_file_path_command(
    mut values: impl Iterator<Item = String>,
    command: FileCommand,
    path_usage: &str,
) -> Result<FileArgs, CliError> {
    let path = required_arg(&mut values, path_usage)?;
    no_extra_args(values)?;
    Ok(FileArgs { command, path })
}

pub(crate) fn parse_schedule_args(args: Vec<String>) -> Result<ScheduleArgs, CliError> {
    let mut values = args.into_iter();
    let command = required_arg(
        &mut values,
        "schedule requires status, advance, claim, or result",
    )?;
    match command.as_str() {
        "status" => {
            let path = required_arg(&mut values, "schedule status requires context/plan.json")?;
            Ok(ScheduleArgs::Status {
                path,
                done: parse_schedule_done_flags(
                    values,
                    "schedule status --done requires a node id",
                )?,
            })
        }
        "advance" => {
            let path = required_arg(&mut values, "schedule advance requires context/plan.json")?;
            Ok(ScheduleArgs::Advance {
                path,
                done: parse_schedule_done_flags(
                    values,
                    "schedule advance --done requires a node id",
                )?,
            })
        }
        "claim" => {
            let path = required_arg(&mut values, "schedule claim requires context/plan.json")?;
            let child = required_arg(&mut values, "schedule claim requires a child name")?;
            no_extra_args(values)?;
            Ok(ScheduleArgs::Claim { path, child })
        }
        "result" => {
            let path = required_arg(&mut values, "schedule result requires context/plan.json")?;
            let child = required_arg(&mut values, "schedule result requires a child name")?;
            let status = required_arg(
                &mut values,
                "schedule result requires done, error, or cancelled",
            )?;
            let status = match status.as_str() {
                "done" => ChildContextStatus::Done,
                "error" => ChildContextStatus::Error,
                "cancelled" => ChildContextStatus::Cancelled,
                _ => {
                    return Err(CliError::usage(
                        "schedule result status expects done, error, or cancelled",
                    ));
                }
            };
            let result = required_arg(&mut values, "schedule result requires result text")?;
            let mut refs_jsonl = String::new();
            while let Some(value) = values.next() {
                match value.as_str() {
                    "--refs-jsonl" => {
                        refs_jsonl = required_arg(
                            &mut values,
                            "schedule result --refs-jsonl requires JSONL text",
                        )?;
                    }
                    _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
                }
            }
            Ok(ScheduleArgs::Result {
                path,
                child,
                status,
                result,
                refs_jsonl,
            })
        }
        _ => Err(CliError::usage(
            "schedule expects status, advance, claim, or result",
        )),
    }
}

pub(crate) fn parse_schedule_done_flags(
    mut values: impl Iterator<Item = String>,
    missing_value: &str,
) -> Result<Vec<String>, CliError> {
    let mut done = Vec::new();
    while let Some(value) = values.next() {
        match value.as_str() {
            "--done" => done.push(required_arg(&mut values, missing_value)?),
            _ => return Err(CliError::usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok(done)
}

pub(crate) fn no_extra_args(mut values: impl Iterator<Item = String>) -> Result<(), CliError> {
    values.next().map_or(Ok(()), |value| {
        Err(CliError::usage(format!("unexpected argument: {value}")))
    })
}
