use serde::Deserialize;
use serde_json::Value;

pub use crate::context::jsonl::{
    ContextJsonlIssue, ContextJsonlKind, ContextJsonlReport, inspect_context_jsonl,
};
pub use crate::message_stream::{
    MessageStreamIssue, MessageStreamReport, inspect_message_stream_jsonl,
};
use crate::{JsonStringField, JsonU64Field, is_json_u64, is_object_name};

/// Canonical JSONL event stream validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventStreamIssue {
    /// Line is not valid JSON.
    InvalidJson(usize),
    /// Event line is not a JSON object.
    EventNotObject(usize),
    /// Event does not have a `type` string.
    MissingType(usize),
    /// Event `type` is not in the stable v1 event set.
    UnknownType { line: usize, event_type: String },
    /// Event type requires a string `run` field.
    MissingRun(usize),
    /// Event contains a provider-native state field.
    ProviderNativeField { line: usize, field: String },
    /// Error event does not use a stable errno `code`.
    InvalidErrorCode(usize),
    /// Done event has an invalid `status`.
    InvalidDoneStatus(usize),
    /// Usage event lacks numeric token counts.
    InvalidUsage(usize),
    /// Tool call event lacks stable tool-call syntax.
    InvalidToolCall(usize),
    /// Agent lifecycle event lacks stable child-agent syntax.
    InvalidAgentLifecycle(usize),
}

/// Result of inspecting a canonical JSONL event stream.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventStreamReport {
    issues: Vec<EventStreamIssue>,
}

impl_issue_report!(EventStreamReport, EventStreamIssue);

/// Inspects a model or agent canonical JSONL event stream.
#[must_use]
pub fn inspect_event_stream_jsonl(content: &str) -> EventStreamReport {
    let mut issues = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        inspect_event_stream_line(line_number, line, &mut issues);
    }
    EventStreamReport::new(issues)
}

fn inspect_event_stream_line(line_number: usize, line: &str, issues: &mut Vec<EventStreamIssue>) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        issues.push(EventStreamIssue::InvalidJson(line_number));
        return;
    };
    let Ok(event) = serde_path_to_error::deserialize::<_, EventLineJson>(
        &mut serde_json::Deserializer::from_str(line),
    ) else {
        issues.push(EventStreamIssue::EventNotObject(line_number));
        return;
    };

    append_provider_native_field_issues(line_number, &value, issues);

    let Some(event_type) = event.event_type.as_ref().and_then(JsonStringField::as_str) else {
        issues.push(EventStreamIssue::MissingType(line_number));
        return;
    };
    if !is_canonical_event_type(event_type) {
        issues.push(EventStreamIssue::UnknownType {
            line: line_number,
            event_type: event_type.to_owned(),
        });
        return;
    }
    if event_requires_run(event_type)
        && event
            .run
            .as_ref()
            .and_then(JsonStringField::as_str)
            .is_none()
    {
        issues.push(EventStreamIssue::MissingRun(line_number));
    }

    match event_type {
        "error" => inspect_error_event(line_number, &event, issues),
        "done" => inspect_done_event(line_number, &event, issues),
        "usage" => inspect_usage_event(line_number, &event, issues),
        "tool_call" => inspect_tool_call_event(line_number, &event, issues),
        "agent.child.cancel" => inspect_agent_child_cancel_event(line_number, &event, issues),
        "agent.stop" => inspect_agent_stop_event(line_number, &event, issues),
        _ => {}
    }
}

#[derive(Deserialize)]
struct EventLineJson {
    #[serde(rename = "type")]
    event_type: Option<JsonStringField>,
    run: Option<JsonStringField>,
    code: Option<JsonStringField>,
    status: Option<JsonStringField>,
    input_tokens: Option<JsonU64Field>,
    output_tokens: Option<JsonU64Field>,
    id: Option<JsonStringField>,
    name: Option<JsonStringField>,
    parent: Option<JsonStringField>,
    child: Option<JsonStringField>,
    reason: Option<JsonStringField>,
    agent: Option<JsonStringField>,
}

fn is_canonical_event_type(value: &str) -> bool {
    matches!(
        value,
        "start"
            | "delta"
            | "message"
            | "reasoning_delta"
            | "reasoning_message"
            | "tool_call"
            | "usage"
            | "error"
            | "done"
            | "agent.child.cancel"
            | "agent.stop"
    )
}

fn event_requires_run(event_type: &str) -> bool {
    !matches!(event_type, "agent.child.cancel" | "agent.stop")
}

fn append_provider_native_field_issues(
    line_number: usize,
    value: &Value,
    issues: &mut Vec<EventStreamIssue>,
) {
    for field in crate::provider_native_fields(value) {
        issues.push(EventStreamIssue::ProviderNativeField {
            line: line_number,
            field: field.to_owned(),
        });
    }
}

fn inspect_error_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    let Some(code) = event.code.as_ref().and_then(JsonStringField::as_str) else {
        issues.push(EventStreamIssue::InvalidErrorCode(line_number));
        return;
    };
    if !is_stable_errno(code) {
        issues.push(EventStreamIssue::InvalidErrorCode(line_number));
    }
}

fn inspect_done_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    if !matches!(
        event.status.as_ref().and_then(JsonStringField::as_str),
        Some("ok" | "error" | "cancelled")
    ) {
        issues.push(EventStreamIssue::InvalidDoneStatus(line_number));
    }
}

fn inspect_usage_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    if !is_json_u64(event.input_tokens.as_ref()) || !is_json_u64(event.output_tokens.as_ref()) {
        issues.push(EventStreamIssue::InvalidUsage(line_number));
    }
}

fn inspect_tool_call_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    let valid_id = event
        .id
        .as_ref()
        .and_then(JsonStringField::as_str)
        .is_some_and(is_object_name);
    let valid_name = event
        .name
        .as_ref()
        .and_then(JsonStringField::as_str)
        .is_some_and(is_object_name);
    if !valid_id || !valid_name {
        issues.push(EventStreamIssue::InvalidToolCall(line_number));
    }
}

fn inspect_agent_child_cancel_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    let parent = event.parent.as_ref().and_then(JsonStringField::as_str);
    let child = event.child.as_ref().and_then(JsonStringField::as_str);
    let reason = event.reason.as_ref().and_then(JsonStringField::as_str);
    if !parent.is_some_and(is_object_name)
        || !child.is_some_and(is_object_name)
        || reason != Some("parent_dead")
    {
        issues.push(EventStreamIssue::InvalidAgentLifecycle(line_number));
    }
}

fn inspect_agent_stop_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    let agent = event.agent.as_ref().and_then(JsonStringField::as_str);
    let status = event.status.as_ref().and_then(JsonStringField::as_str);
    if !agent.is_some_and(is_object_name) || status != Some("cancelled") {
        issues.push(EventStreamIssue::InvalidAgentLifecycle(line_number));
    }
}

fn is_stable_errno(code: &str) -> bool {
    matches!(
        code,
        "EACCES"
            | "EINVAL"
            | "ENOENT"
            | "EMSGSIZE"
            | "EHOSTDOWN"
            | "ECONNREFUSED"
            | "EAGAIN"
            | "EIO"
            | "EINTR"
            | "ENOSYS"
    )
}
