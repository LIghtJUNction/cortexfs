//! Structural validation and durable write helpers for ATIF trajectories.

use std::fmt;
use std::path::Path;

use crate::support::atomic::atomic_replace_text;

use super::types::{Trajectory, TrajectoryStep};

const MAX_TRAJECTORY_ISSUE_FIELD_CHARS: usize = 128;

/// Structural issue found while validating an ATIF trajectory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrajectoryIssue {
    /// `schema_version` is missing or not an ATIF version string.
    InvalidSchemaVersion,
    /// `agent.name` is empty.
    MissingAgentName,
    /// `agent.version` is empty.
    MissingAgentVersion,
    /// A step has a non-positive or non-sequential `step_id`.
    InvalidStepId {
        /// Zero-based index into `steps`.
        index: usize,
        /// Observed `step_id`.
        step_id: u64,
    },
    /// A step `source` is not `system`, `user`, or `agent`.
    InvalidStepSource {
        /// Zero-based index into `steps`.
        index: usize,
        /// Observed source value.
        source: String,
    },
    /// A tool call is missing required identity fields.
    InvalidToolCall {
        /// Zero-based index into `steps`.
        step_index: usize,
        /// Zero-based index into `tool_calls`.
        call_index: usize,
    },
    /// An observation result is empty (no content and no `source_call_id`).
    EmptyObservationResult {
        /// Zero-based index into `steps`.
        step_index: usize,
        /// Zero-based index into `observation.results`.
        result_index: usize,
    },
    /// An observation references no tool call on its current step.
    UnknownObservationSourceCall {
        /// Zero-based index into `steps`.
        step_index: usize,
        /// Zero-based index into `observation.results`.
        result_index: usize,
        /// Referenced tool-call id.
        source_call_id: String,
    },
}

impl fmt::Display for TrajectoryIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::InvalidSchemaVersion => f.write_str("invalid schema version"),
            Self::MissingAgentName => f.write_str("missing agent name"),
            Self::MissingAgentVersion => f.write_str("missing agent version"),
            Self::InvalidStepId { index, step_id } => write!(
                f,
                "invalid step id: step={} step_id={step_id}",
                index.saturating_add(1)
            ),
            Self::InvalidStepSource { index, ref source } => {
                let source = safe_issue_field(source);
                write!(
                    f,
                    "invalid step source: step={} source={source}",
                    index.saturating_add(1)
                )
            }
            Self::InvalidToolCall {
                step_index,
                call_index,
            } => write!(
                f,
                "invalid tool call: step={} call={}",
                step_index.saturating_add(1),
                call_index.saturating_add(1)
            ),
            Self::EmptyObservationResult {
                step_index,
                result_index,
            } => write!(
                f,
                "empty observation result: step={} result={}",
                step_index.saturating_add(1),
                result_index.saturating_add(1)
            ),
            Self::UnknownObservationSourceCall {
                step_index,
                result_index,
                ref source_call_id,
            } => {
                let source_call_id = safe_issue_field(source_call_id);
                write!(
                    f,
                    "unknown observation source call: step={} result={} call_id={source_call_id}",
                    step_index.saturating_add(1),
                    result_index.saturating_add(1)
                )
            }
        }
    }
}

fn safe_issue_field(value: &str) -> String {
    let mut safe = String::new();
    let mut characters = 0_usize;
    let mut truncated = false;
    for character in value.chars() {
        let escaped = character.escape_default().to_string();
        let escaped_characters = escaped.chars().count();
        if characters.saturating_add(escaped_characters) > MAX_TRAJECTORY_ISSUE_FIELD_CHARS - 3 {
            truncated = true;
            break;
        }
        safe.push_str(&escaped);
        characters = characters.saturating_add(escaped_characters);
    }
    if truncated {
        safe.push_str("...");
    }
    safe
}

/// Result of inspecting an ATIF trajectory document.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrajectoryReport {
    issues: Vec<TrajectoryIssue>,
}

impl_issue_report!(TrajectoryReport, TrajectoryIssue);

/// Validates required ATIF structural invariants used by `CortexFS` exporters.
#[must_use]
pub fn validate_trajectory(trajectory: &Trajectory) -> TrajectoryReport {
    let mut issues = Vec::new();
    if !trajectory.schema_version.starts_with("ATIF-v") {
        issues.push(TrajectoryIssue::InvalidSchemaVersion);
    }
    if trajectory.agent.name.trim().is_empty() {
        issues.push(TrajectoryIssue::MissingAgentName);
    }
    if trajectory.agent.version.trim().is_empty() {
        issues.push(TrajectoryIssue::MissingAgentVersion);
    }

    for (index, step) in trajectory.steps.iter().enumerate() {
        let expected = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
        if step.step_id != expected {
            issues.push(TrajectoryIssue::InvalidStepId {
                index,
                step_id: step.step_id,
            });
        }
        if !is_valid_step_source(&step.source) {
            issues.push(TrajectoryIssue::InvalidStepSource {
                index,
                source: step.source.clone(),
            });
        }
        inspect_step_tool_calls(index, step, &mut issues);
        inspect_step_observation(index, step, &mut issues);
    }

    TrajectoryReport::new(issues)
}

/// Serializes a trajectory as pretty JSON and atomically replaces `path`.
pub fn write_trajectory_json(path: &Path, trajectory: &Trajectory) -> std::io::Result<()> {
    let content = serde_json::to_string_pretty(trajectory)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut body = content;
    if !body.ends_with('\n') {
        body.push('\n');
    }
    atomic_replace_text(path, &body)
}

fn is_valid_step_source(source: &str) -> bool {
    matches!(source, "system" | "user" | "agent")
}

fn inspect_step_tool_calls(
    step_index: usize,
    step: &TrajectoryStep,
    issues: &mut Vec<TrajectoryIssue>,
) {
    let Some(calls) = step.tool_calls.as_ref() else {
        return;
    };
    for (call_index, call) in calls.iter().enumerate() {
        if call.tool_call_id.trim().is_empty() || call.function_name.trim().is_empty() {
            issues.push(TrajectoryIssue::InvalidToolCall {
                step_index,
                call_index,
            });
        }
    }
}

fn inspect_step_observation(
    step_index: usize,
    step: &TrajectoryStep,
    issues: &mut Vec<TrajectoryIssue>,
) {
    let Some(observation) = step.observation.as_ref() else {
        return;
    };
    for (result_index, result) in observation.results.iter().enumerate() {
        let has_id = result
            .source_call_id
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        let has_content = result
            .content
            .as_ref()
            .is_some_and(|value| !value.is_empty());
        if !has_id && !has_content {
            issues.push(TrajectoryIssue::EmptyObservationResult {
                step_index,
                result_index,
            });
        }
        if let Some(source_call_id) = result
            .source_call_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && !step.tool_calls.as_ref().is_some_and(|calls| {
                calls
                    .iter()
                    .any(|call| call.tool_call_id.trim() == source_call_id)
            })
        {
            issues.push(TrajectoryIssue::UnknownObservationSourceCall {
                step_index,
                result_index,
                source_call_id: source_call_id.to_owned(),
            });
        }
    }
}
