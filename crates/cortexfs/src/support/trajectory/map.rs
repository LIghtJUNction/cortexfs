//! Pure projection from `CortexFS` session JSONL into an ATIF trajectory.

use std::collections::VecDeque;
use std::path::Path;

use serde_json::{Map, Value};

use crate::agent::prompt::message_content_text;
use crate::support::columnar::{HistoryGuard, Stream};
use crate::support::plain::{path_metadata_no_follow, read_small_text_file};
use crate::{JsonlLineShape, for_each_jsonl_line, parse_jsonl_line};

use super::types::{
    ATIF_SCHEMA_VERSION, TRAJECTORY_DEFAULT_AGENT_NAME, Trajectory, TrajectoryAgent,
    TrajectoryFinalMetrics, TrajectoryMetrics, TrajectoryObservation, TrajectoryObservationResult,
    TrajectoryStep, TrajectoryToolCall,
};

/// Upper bound for reading durable session files during ATIF projection.
pub const MAX_TRAJECTORY_SESSION_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Error while mapping a durable session directory into ATIF.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrajectoryMapError {
    /// `messages.jsonl` is missing or not a plain file.
    MissingMessages,
    /// `events.jsonl` is missing or not a plain file.
    MissingEvents,
    /// A required session file could not be read as UTF-8 text.
    CannotRead(&'static str),
}

/// Builds an ATIF trajectory from a durable session directory.
///
/// Reads `messages.jsonl`, `events.jsonl`, and optional `meta.json`. Does not
/// write files and does not mutate session history.
pub fn trajectory_from_session_dir(session_dir: &Path) -> Result<Trajectory, TrajectoryMapError> {
    for (file, missing) in [
        ("messages.jsonl", TrajectoryMapError::MissingMessages),
        ("events.jsonl", TrajectoryMapError::MissingEvents),
    ] {
        match path_metadata_no_follow(&session_dir.join(file)) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Err(missing),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(missing),
            Err(_error) => return Err(TrajectoryMapError::CannotRead(file)),
        }
    }
    let history = HistoryGuard::shared(session_dir)
        .map_err(|_error| TrajectoryMapError::CannotRead("messages.jsonl"))?;
    let messages = read_session_text(
        &history,
        Stream::Messages,
        "messages.jsonl",
        TrajectoryMapError::MissingMessages,
    )?;
    let events = read_session_text(
        &history,
        Stream::Events,
        "events.jsonl",
        TrajectoryMapError::MissingEvents,
    )?;
    let meta = read_optional_session_text(session_dir, "meta.json")?;
    let session_id = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    Ok(trajectory_from_session_jsonl(
        &messages,
        &events,
        meta.as_deref(),
        session_id.as_deref(),
    ))
}

/// Pure map from session JSONL text into an ATIF trajectory.
#[must_use]
pub fn trajectory_from_session_jsonl(
    messages_jsonl: &str,
    events_jsonl: &str,
    meta_json: Option<&str>,
    session_id: Option<&str>,
) -> Trajectory {
    let meta = parse_meta(meta_json);
    let indexed = index_events(events_jsonl);
    let (mut steps, remaining_tool_calls) =
        map_messages_to_steps(messages_jsonl, indexed.tool_calls);
    attach_orphan_tool_calls(&mut steps, &remaining_tool_calls);
    attach_run_metrics(&mut steps, &indexed.usages);
    let final_metrics = build_final_metrics(&steps, &indexed.usages);

    let mut agent_extra = Map::new();
    if let Some(scope) = meta.scope {
        agent_extra.insert("scope".to_owned(), Value::String(scope));
    }

    Trajectory {
        schema_version: ATIF_SCHEMA_VERSION.to_owned(),
        session_id: session_id.map(str::to_owned),
        agent: TrajectoryAgent {
            name: meta
                .client
                .unwrap_or_else(|| TRAJECTORY_DEFAULT_AGENT_NAME.to_owned()),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            model_name: meta.model,
            extra: if agent_extra.is_empty() {
                None
            } else {
                Some(agent_extra)
            },
        },
        steps,
        final_metrics,
        notes: Some("projected from CortexFS session messages.jsonl + events.jsonl".to_owned()),
        extra: None,
    }
}

struct SessionMeta {
    client: Option<String>,
    model: Option<String>,
    scope: Option<String>,
}

struct IndexedEvents {
    tool_calls: VecDeque<RunToolCall>,
    usages: Vec<(Option<String>, TrajectoryMetrics)>,
}

#[derive(Clone)]
struct RunToolCall {
    run: Option<String>,
    call: TrajectoryToolCall,
}

fn read_session_text(
    history: &HistoryGuard<'_>,
    stream: Stream,
    file: &'static str,
    missing: TrajectoryMapError,
) -> Result<String, TrajectoryMapError> {
    match history.read_text(stream, MAX_TRAJECTORY_SESSION_FILE_BYTES) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(missing),
        Err(_error) => Err(TrajectoryMapError::CannotRead(file)),
    }
}

fn read_optional_session_text(
    session_dir: &Path,
    file: &'static str,
) -> Result<Option<String>, TrajectoryMapError> {
    let path = session_dir.join(file);
    match read_small_text_file(&path, MAX_TRAJECTORY_SESSION_FILE_BYTES) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_error) => Err(TrajectoryMapError::CannotRead(file)),
    }
}

fn parse_meta(meta_json: Option<&str>) -> SessionMeta {
    let Some(raw) = meta_json.map(str::trim).filter(|value| !value.is_empty()) else {
        return SessionMeta {
            client: None,
            model: None,
            scope: None,
        };
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return SessionMeta {
            client: None,
            model: None,
            scope: None,
        };
    };
    SessionMeta {
        client: value
            .get("client")
            .and_then(Value::as_str)
            .map(str::to_owned),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        scope: value
            .get("scope")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn index_events(events_jsonl: &str) -> IndexedEvents {
    let mut tool_calls = VecDeque::new();
    let mut usages = Vec::new();
    for_each_jsonl_line(events_jsonl, |_line_number, line| {
        let JsonlLineShape::Value(value) = parse_jsonl_line(line) else {
            return;
        };
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            return;
        };
        match event_type {
            "tool_call" => {
                if let Some(call) = tool_call_from_event(&value) {
                    tool_calls.push_back(RunToolCall {
                        run: event_run(&value),
                        call,
                    });
                }
            }
            "usage" => {
                let run = value.get("run").and_then(Value::as_str).map(str::to_owned);
                let metrics = TrajectoryMetrics {
                    prompt_tokens: value.get("input_tokens").and_then(Value::as_u64),
                    completion_tokens: value.get("output_tokens").and_then(Value::as_u64),
                };
                if metrics.prompt_tokens.is_some() || metrics.completion_tokens.is_some() {
                    usages.push((run, metrics));
                }
            }
            _ => {}
        }
    });
    IndexedEvents { tool_calls, usages }
}

fn tool_call_from_event(value: &Value) -> Option<TrajectoryToolCall> {
    let tool_call_id = value.get("id").and_then(Value::as_str)?.to_owned();
    let function_name = value.get("name").and_then(Value::as_str)?.to_owned();
    let arguments = value.get("arguments").map_or_else(Map::new, |arguments| {
        arguments.as_object().map_or_else(
            || {
                let mut map = Map::new();
                map.insert("value".to_owned(), arguments.clone());
                map
            },
            Map::clone,
        )
    });
    Some(TrajectoryToolCall {
        tool_call_id,
        function_name,
        arguments,
    })
}

fn map_messages_to_steps(
    messages_jsonl: &str,
    mut pending_tool_calls: VecDeque<RunToolCall>,
) -> (Vec<TrajectoryStep>, VecDeque<RunToolCall>) {
    let mut steps = Vec::new();
    let mut step_id = 1_u64;

    for_each_jsonl_line(messages_jsonl, |_line_number, line| {
        let JsonlLineShape::Value(value) = parse_jsonl_line(line) else {
            return;
        };
        let Some(role) = value.get("role").and_then(Value::as_str) else {
            return;
        };
        match role {
            "user" | "system" => {
                let source = if role == "user" { "user" } else { "system" };
                steps.push(TrajectoryStep {
                    step_id,
                    source: source.to_owned(),
                    message: message_content_text(value.get("content")),
                    tool_calls: None,
                    observation: None,
                    metrics: None,
                    extra: None,
                });
                step_id = step_id.saturating_add(1);
            }
            "assistant" => {
                let run = event_run(&value);
                steps.push(TrajectoryStep {
                    step_id,
                    source: "agent".to_owned(),
                    message: message_content_text(value.get("content")),
                    tool_calls: None,
                    observation: None,
                    metrics: None,
                    extra: run.as_deref().map(run_extra),
                });
                step_id = step_id.saturating_add(1);
            }
            "tool" => {
                let Some(message_run) = event_run(&value) else {
                    return;
                };
                let mut results = tool_observation_results(&value);
                let call_ids: Vec<String> = results
                    .iter()
                    .filter_map(|result| result.source_call_id.clone())
                    .collect();
                let matched_calls =
                    take_matching_tool_calls(&mut pending_tool_calls, &call_ids, &message_run);
                results.retain(|result| {
                    result
                        .source_call_id
                        .as_deref()
                        .is_some_and(|source_call_id| {
                            matched_calls
                                .iter()
                                .any(|call| call.call.tool_call_id == source_call_id)
                        })
                });
                if results.is_empty() && matched_calls.is_empty() {
                    return;
                }
                if let Some(index) = agent_step_for_run(&steps, Some(&message_run)) {
                    let Some(last) = steps.get_mut(index) else {
                        return;
                    };
                    if !matched_calls.is_empty() {
                        merge_tool_calls(last, calls_without_runs(matched_calls));
                    }
                    merge_observation(last, results);
                } else {
                    // Tool results can land before any assistant text message when
                    // the model only emitted tool_call events. Project them as an
                    // agent step so tool_calls stay on a valid ATIF source.
                    let mut step = TrajectoryStep {
                        step_id,
                        source: "agent".to_owned(),
                        message: String::new(),
                        tool_calls: None,
                        observation: None,
                        metrics: None,
                        extra: Some(run_extra(&message_run)),
                    };
                    if !matched_calls.is_empty() {
                        merge_tool_calls(&mut step, calls_without_runs(matched_calls));
                    }
                    merge_observation(&mut step, results);
                    steps.push(step);
                    step_id = step_id.saturating_add(1);
                }
            }
            _ => {}
        }
    });

    (steps, pending_tool_calls)
}

fn tool_observation_results(message: &Value) -> Vec<TrajectoryObservationResult> {
    let Some(parts) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
        .map(|part| TrajectoryObservationResult {
            source_call_id: part
                .get("tool_call_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            content: part
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
        .collect()
}

fn take_matching_tool_calls(
    pending: &mut VecDeque<RunToolCall>,
    call_ids: &[String],
    run: &str,
) -> Vec<RunToolCall> {
    if call_ids.is_empty() {
        return Vec::new();
    }
    let mut matched = Vec::with_capacity(call_ids.len());
    for call_id in call_ids {
        let position = pending.iter().position(|call| {
            call.call.tool_call_id == *call_id && call.run.as_deref() == Some(run)
        });
        if let Some(position) = position
            && let Some(call) = pending.remove(position)
        {
            matched.push(call);
        }
    }
    matched
}

fn merge_tool_calls(step: &mut TrajectoryStep, calls: Vec<TrajectoryToolCall>) {
    match step.tool_calls.as_mut() {
        Some(existing) => existing.extend(calls),
        None => step.tool_calls = Some(calls),
    }
}

fn merge_observation(step: &mut TrajectoryStep, results: Vec<TrajectoryObservationResult>) {
    if results.is_empty() {
        return;
    }
    match step.observation.as_mut() {
        Some(observation) => observation.results.extend(results),
        None => {
            step.observation = Some(TrajectoryObservation { results });
        }
    }
}

fn attach_orphan_tool_calls(steps: &mut Vec<TrajectoryStep>, remaining: &VecDeque<RunToolCall>) {
    if remaining.is_empty() {
        return;
    }
    let mut groups: Vec<(Option<String>, Vec<TrajectoryToolCall>)> = Vec::new();
    for orphan in remaining {
        if let Some(group) = groups.iter_mut().find(|group| group.0 == orphan.run) {
            group.1.push(orphan.call.clone());
        } else {
            groups.push((orphan.run.clone(), vec![orphan.call.clone()]));
        }
    }
    for (run, calls) in groups {
        if let Some(index) = agent_step_for_run(steps, run.as_deref())
            && let Some(step) = steps.get_mut(index)
        {
            merge_tool_calls(step, calls);
            continue;
        }
        let step_id = u64::try_from(steps.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        steps.push(TrajectoryStep {
            step_id,
            source: "agent".to_owned(),
            message: String::new(),
            tool_calls: Some(calls),
            observation: None,
            metrics: None,
            extra: run.as_deref().map(run_extra),
        });
    }
}

fn attach_run_metrics(
    steps: &mut [TrajectoryStep],
    usages: &[(Option<String>, TrajectoryMetrics)],
) {
    let agent_indices: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| (step.source == "agent").then_some(index))
        .collect();
    if agent_indices.is_empty() || usages.is_empty() {
        return;
    }
    for usage in usages {
        if let Some(run) = usage.0.as_deref()
            && let Some(index) = unique_agent_step_for_run(steps, run)
            && let Some(step) = steps.get_mut(index)
        {
            merge_step_metrics(step, usage.1);
        }
    }
    if usages.len() == 1
        && agent_indices.len() == 1
        && let Some(&agent_index) = agent_indices.first()
        && steps
            .get(agent_index)
            .is_some_and(|step| step.metrics.is_none())
        && let Some(usage) = usages.first()
        && let Some(step) = steps.get_mut(agent_index)
    {
        step.metrics = Some(usage.1);
    }
}

fn unique_agent_step_for_run(steps: &[TrajectoryStep], run: &str) -> Option<usize> {
    let mut matches = steps.iter().enumerate().filter_map(|(index, step)| {
        (step.source == "agent" && step_run(step) == Some(run)).then_some(index)
    });
    let only = matches.next()?;
    matches.next().is_none().then_some(only)
}

fn event_run(value: &Value) -> Option<String> {
    value.get("run").and_then(Value::as_str).map(str::to_owned)
}

fn run_extra(run: &str) -> Map<String, Value> {
    let mut extra = Map::new();
    extra.insert("run".to_owned(), Value::String(run.to_owned()));
    extra
}

fn step_run(step: &TrajectoryStep) -> Option<&str> {
    step.extra
        .as_ref()
        .and_then(|extra| extra.get("run"))
        .and_then(Value::as_str)
}

fn agent_step_for_run(steps: &[TrajectoryStep], run: Option<&str>) -> Option<usize> {
    if let Some(run) = run {
        return steps.iter().enumerate().rev().find_map(|(index, step)| {
            (step.source == "agent" && step_run(step) == Some(run)).then_some(index)
        });
    }
    let mut agents = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| (step.source == "agent").then_some(index));
    let only = agents.next()?;
    agents.next().is_none().then_some(only)
}

fn calls_without_runs(calls: Vec<RunToolCall>) -> Vec<TrajectoryToolCall> {
    calls.into_iter().map(|call| call.call).collect()
}

fn merge_step_metrics(step: &mut TrajectoryStep, metrics: TrajectoryMetrics) {
    let Some(existing) = step.metrics.as_mut() else {
        step.metrics = Some(metrics);
        return;
    };
    existing.prompt_tokens = match (existing.prompt_tokens, metrics.prompt_tokens) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (left, right) => left.or(right),
    };
    existing.completion_tokens = match (existing.completion_tokens, metrics.completion_tokens) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (left, right) => left.or(right),
    };
}

fn build_final_metrics(
    steps: &[TrajectoryStep],
    usages: &[(Option<String>, TrajectoryMetrics)],
) -> Option<TrajectoryFinalMetrics> {
    let mut total_prompt = 0_u64;
    let mut total_completion = 0_u64;
    let mut saw_prompt = false;
    let mut saw_completion = false;

    for usage in usages {
        let metrics = &usage.1;
        if let Some(value) = metrics.prompt_tokens {
            total_prompt = total_prompt.saturating_add(value);
            saw_prompt = true;
        }
        if let Some(value) = metrics.completion_tokens {
            total_completion = total_completion.saturating_add(value);
            saw_completion = true;
        }
    }
    if usages.is_empty() {
        for step in steps {
            let Some(metrics) = step.metrics.as_ref() else {
                continue;
            };
            if let Some(value) = metrics.prompt_tokens {
                total_prompt = total_prompt.saturating_add(value);
                saw_prompt = true;
            }
            if let Some(value) = metrics.completion_tokens {
                total_completion = total_completion.saturating_add(value);
                saw_completion = true;
            }
        }
    }

    if !saw_prompt && !saw_completion && steps.is_empty() {
        return None;
    }
    Some(TrajectoryFinalMetrics {
        total_prompt_tokens: saw_prompt.then_some(total_prompt),
        total_completion_tokens: saw_completion.then_some(total_completion),
        total_steps: u64::try_from(steps.len()).ok(),
    })
}
