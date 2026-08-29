use std::path::PathBuf;

use crate::{AppError, AppResult};

const USAGE: &str = "usage: cortexfs-futureagi <export|evaluate> --trajectory PATH [options]";

#[derive(Debug)]
pub(crate) enum Command {
    Export(ExportOptions),
    Evaluate(EvaluateOptions),
}

#[derive(Debug)]
pub(crate) struct ExportOptions {
    pub(crate) trajectory: PathBuf,
    pub(crate) include_context: bool,
}

#[derive(Debug)]
pub(crate) struct EvaluateOptions {
    pub(crate) trajectory: PathBuf,
    pub(crate) eval: String,
    pub(crate) include_context: bool,
    pub(crate) base_url: Option<String>,
    pub(crate) timeout: u64,
}

pub(crate) fn parse(mut values: impl Iterator<Item = String>) -> AppResult<Command> {
    let command = values.next().ok_or_else(|| AppError::new(USAGE))?;
    let rest = values.collect::<Vec<_>>();
    if rest.iter().any(|value| value == "--help" || value == "-h") {
        return Err(AppError::new(USAGE));
    }
    match command.as_str() {
        "export" => parse_export(rest),
        "evaluate" => parse_evaluate(rest),
        _ => Err(AppError::new(format!(
            "unknown command `{command}`\n{USAGE}"
        ))),
    }
}

fn parse_export(values: Vec<String>) -> AppResult<Command> {
    let (trajectory, include_context, extra) = parse_common(values)?;
    if let Some(value) = extra.first() {
        return Err(AppError::new(format!("unexpected argument `{value}`")));
    }
    Ok(Command::Export(ExportOptions {
        trajectory,
        include_context,
    }))
}

fn parse_evaluate(values: Vec<String>) -> AppResult<Command> {
    let (trajectory, include_context, mut extra) = parse_common(values)?;
    let eval = take_value(&mut extra, "--eval")?
        .ok_or_else(|| AppError::new("evaluate requires --eval NAME"))?;
    let base_url = take_value(&mut extra, "--base-url")?;
    let timeout = take_value(&mut extra, "--timeout")?
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_error| AppError::new("--timeout must be an integer"))?
        .unwrap_or(200);
    if let Some(value) = extra.first() {
        return Err(AppError::new(format!("unexpected argument `{value}`")));
    }
    Ok(Command::Evaluate(EvaluateOptions {
        trajectory,
        eval,
        include_context,
        base_url,
        timeout,
    }))
}

fn parse_common(mut values: Vec<String>) -> AppResult<(PathBuf, bool, Vec<String>)> {
    let include_context = values.iter().any(|value| value == "--include-context");
    values.retain(|value| value != "--include-context");
    let trajectory = take_value(&mut values, "--trajectory")?
        .ok_or_else(|| AppError::new("--trajectory PATH is required"))?;
    Ok((PathBuf::from(trajectory), include_context, values))
}

fn take_value(values: &mut Vec<String>, flag: &str) -> AppResult<Option<String>> {
    let Some(index) = values.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    values.remove(index);
    if index >= values.len() {
        return Err(AppError::new(format!("{flag} requires a value")));
    }
    Ok(Some(values.remove(index)))
}
