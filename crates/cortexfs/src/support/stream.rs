use crate::*;

use serde::Deserialize;
use serde_json::Value;

pub use crate::context::jsonl::{
    ContextJsonlIssue, ContextJsonlKind, ContextJsonlReport, inspect_context_jsonl,
};
pub use crate::support::message::{
    MessageStreamIssue, MessageStreamReport, inspect_message_stream_jsonl,
};
use crate::{
    JsonStringField, JsonU64Field, JsonlLineShape, for_each_jsonl_line, is_json_u64,
    is_object_name, parse_jsonl_line,
};

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
    /// Host approval event lacks stable approval syntax.
    InvalidApproval(usize),
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
    for_each_jsonl_line(content, |line_number, line| {
        inspect_event_stream_line(line_number, line, &mut issues);
    });
    EventStreamReport::new(issues)
}

pub(crate) fn inspect_event_stream_line(
    line_number: usize,
    line: &str,
    issues: &mut Vec<EventStreamIssue>,
) {
    let JsonlLineShape::Value(value) = parse_jsonl_line(line) else {
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
        "approval_request" | "approval_result" => {
            inspect_approval_event(line_number, event_type, &event, issues);
        }
        "agent.child.cancel" => inspect_agent_child_cancel_event(line_number, &event, issues),
        "agent.stop" => inspect_agent_stop_event(line_number, &event, issues),
        _ => {}
    }
}

#[derive(Deserialize)]
pub(crate) struct EventLineJson {
    #[serde(rename = "type")]
    event_type: Option<JsonStringField>,
    run: Option<JsonStringField>,
    code: Option<JsonStringField>,
    status: Option<JsonStringField>,
    input_tokens: Option<JsonU64Field>,
    output_tokens: Option<JsonU64Field>,
    id: Option<JsonStringField>,
    name: Option<JsonStringField>,
    args: Option<Value>,
    decision: Option<JsonStringField>,
    parent: Option<JsonStringField>,
    child: Option<JsonStringField>,
    reason: Option<JsonStringField>,
    agent: Option<JsonStringField>,
}

pub(crate) fn is_canonical_event_type(value: &str) -> bool {
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
            | "approval_request"
            | "approval_result"
            | "agent.child.cancel"
            | "agent.stop"
    )
}

pub(crate) fn event_requires_run(event_type: &str) -> bool {
    !matches!(event_type, "agent.child.cancel" | "agent.stop")
}

pub(crate) fn append_provider_native_field_issues(
    line_number: usize,
    value: &Value,
    issues: &mut Vec<EventStreamIssue>,
) {
    for field in provider_native_fields(value) {
        issues.push(EventStreamIssue::ProviderNativeField {
            line: line_number,
            field: field.to_owned(),
        });
    }
}

pub(crate) fn inspect_error_event(
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

pub(crate) fn inspect_done_event(
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

pub(crate) fn inspect_usage_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    if !is_json_u64(event.input_tokens.as_ref()) || !is_json_u64(event.output_tokens.as_ref()) {
        issues.push(EventStreamIssue::InvalidUsage(line_number));
    }
}

pub(crate) fn inspect_tool_call_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    if !has_valid_id_name(event) {
        issues.push(EventStreamIssue::InvalidToolCall(line_number));
    }
}

pub(crate) fn inspect_approval_event(
    line_number: usize,
    event_type: &str,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    let valid_body = match event_type {
        "approval_request" => event
            .args
            .as_ref()
            .and_then(Value::as_array)
            .is_some_and(|args| args.iter().all(Value::is_string)),
        "approval_result" => {
            matches!(
                event.decision.as_ref().and_then(JsonStringField::as_str),
                Some("allow_once" | "deny")
            ) && event
                .reason
                .as_ref()
                .and_then(JsonStringField::as_str)
                .is_some_and(|reason| !reason.is_empty())
        }
        _ => true,
    };
    if !has_valid_id_name(event) || !valid_body {
        issues.push(EventStreamIssue::InvalidApproval(line_number));
    }
}

pub(crate) fn has_valid_id_name(event: &EventLineJson) -> bool {
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
    valid_id && valid_name
}

pub(crate) fn inspect_agent_child_cancel_event(
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

pub(crate) fn inspect_agent_stop_event(
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

pub(crate) fn is_stable_errno(code: &str) -> bool {
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
