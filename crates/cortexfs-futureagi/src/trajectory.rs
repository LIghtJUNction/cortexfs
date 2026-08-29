use std::fs;
use std::io::{self, Read};
use std::path::Path;

use cortexfs::{Trajectory, validate_trajectory};
use serde::Serialize;

use crate::{AppError, AppResult};

#[derive(Debug, Serialize)]
pub(crate) struct EvalCase {
    input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<String>,
}

pub(crate) fn load(path: &Path) -> AppResult<Trajectory> {
    let mut content = String::new();
    if path == Path::new("-") {
        io::stdin()
            .read_to_string(&mut content)
            .map_err(|error| AppError::new(format!("cannot read stdin: {error}")))?;
    } else {
        content = fs::read_to_string(path)
            .map_err(|error| AppError::new(format!("cannot read {}: {error}", path.display())))?;
    }
    let trajectory: Trajectory = serde_json::from_str(&content)
        .map_err(|error| AppError::new(format!("invalid ATIF JSON: {error}")))?;
    let report = validate_trajectory(&trajectory);
    if !report.is_ok() {
        let issues = report
            .issues()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(AppError::new(format!("invalid ATIF trajectory: {issues}")));
    }
    Ok(trajectory)
}

pub(crate) fn cases(trajectory: &Trajectory, include_context: bool) -> Vec<EvalCase> {
    let input = join_steps(trajectory, "user");
    let input = if input.is_empty() {
        trajectory
            .steps
            .first()
            .map(|step| step.message.clone())
            .unwrap_or_default()
    } else {
        input
    };
    let output = trajectory
        .steps
        .iter()
        .rev()
        .find(|step| step.source == "agent" && !step.message.is_empty())
        .map(|step| step.message.clone());
    let context = include_context
        .then(|| observations(trajectory))
        .filter(|text| !text.is_empty());
    vec![EvalCase {
        input,
        output,
        context,
    }]
}

fn join_steps(trajectory: &Trajectory, source: &str) -> String {
    trajectory
        .steps
        .iter()
        .filter(|step| step.source == source && !step.message.is_empty())
        .map(|step| step.message.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn observations(trajectory: &Trajectory) -> String {
    trajectory
        .steps
        .iter()
        .filter_map(|step| step.observation.as_ref())
        .flat_map(|observation| observation.results.iter())
        .filter_map(|result| result.content.as_deref())
        .filter(|content| !content.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .join("\n\n")
}
