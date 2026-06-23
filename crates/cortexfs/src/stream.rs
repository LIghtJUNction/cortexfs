use serde::Deserialize;
use serde_json::Value;

use crate::{JsonStringField, is_object_name, validate_context_pack_source};

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

/// Canonical durable message history validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageStreamIssue {
    /// Line is not valid JSON.
    InvalidJson(usize),
    /// Message line is not a JSON object.
    MessageNotObject(usize),
    /// Message does not have a stable `role` string.
    MissingRole(usize),
    /// Message `role` is not in the stable v1 role set.
    InvalidRole { line: usize, role: String },
    /// Message does not have `content`.
    MissingContent(usize),
    /// Message `content` is neither a string nor a canonical content-part array.
    InvalidContent(usize),
    /// Message contains a provider-native state field.
    ProviderNativeField { line: usize, field: String },
}

/// Result of inspecting `messages.jsonl`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MessageStreamReport {
    issues: Vec<MessageStreamIssue>,
}

/// Stable context JSONL file kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextJsonlKind {
    /// `context/facts.jsonl`: stable fact records.
    Facts,
    /// `context/decisions.jsonl`: accepted decision records.
    Decisions,
    /// `context/refs.jsonl`: file, artifact, tool output, and swap refs.
    Refs,
    /// `context/swap/index.jsonl`: swapped-out prompt working-set refs.
    SwapIndex,
    /// `context/dedup/index.jsonl`: content-addressed dedup refs.
    DedupIndex,
}

/// Context JSONL validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextJsonlIssue {
    /// Line is not valid JSON.
    InvalidJson(usize),
    /// JSONL record is not a JSON object.
    RecordNotObject(usize),
    /// Required string field is missing or not a string.
    MissingStringField { line: usize, field: String },
    /// Required number field is missing or not an unsigned integer.
    MissingNumberField { line: usize, field: String },
    /// Required string-array field is missing or malformed.
    MissingStringArrayField { line: usize, field: String },
    /// Field value is outside the stable v1 syntax for this file.
    InvalidField {
        /// One-based JSONL line number.
        line: usize,
        /// Field name.
        field: String,
        /// Rejected value.
        value: String,
    },
}

/// Result of inspecting a context JSONL file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextJsonlReport {
    issues: Vec<ContextJsonlIssue>,
}

impl EventStreamReport {
    /// Creates a report with collected event stream issues.
    #[must_use]
    pub const fn new(issues: Vec<EventStreamIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when all events are stable v1 event frames.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected event stream issues.
    #[must_use]
    pub fn issues(&self) -> &[EventStreamIssue] {
        &self.issues
    }
}

impl MessageStreamReport {
    /// Creates a report with collected message stream issues.
    #[must_use]
    pub const fn new(issues: Vec<MessageStreamIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when all messages use stable v1 message frames.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected message stream issues.
    #[must_use]
    pub fn issues(&self) -> &[MessageStreamIssue] {
        &self.issues
    }
}

impl ContextJsonlReport {
    /// Creates a report with collected context JSONL issues.
    #[must_use]
    pub const fn new(issues: Vec<ContextJsonlIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when all records use the stable v1 context shape.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected context JSONL issues.
    #[must_use]
    pub fn issues(&self) -> &[ContextJsonlIssue] {
        &self.issues
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonU64Field {
    Number(u64),
    Other(Value),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonStringArrayField {
    Strings(Vec<String>),
    Other(Value),
}

/// Inspects durable `messages.jsonl` for the canonical v1 role/content shape.
#[must_use]
pub fn inspect_message_stream_jsonl(content: &str) -> MessageStreamReport {
    let mut issues = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        inspect_message_stream_line(line_number, line, &mut issues);
    }
    MessageStreamReport::new(issues)
}

fn inspect_message_stream_line(
    line_number: usize,
    line: &str,
    issues: &mut Vec<MessageStreamIssue>,
) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        issues.push(MessageStreamIssue::InvalidJson(line_number));
        return;
    };
    let Ok(message) = serde_path_to_error::deserialize::<_, MessageLineJson>(
        &mut serde_json::Deserializer::from_str(line),
    ) else {
        issues.push(MessageStreamIssue::MessageNotObject(line_number));
        return;
    };

    append_provider_native_message_field_issues(line_number, &value, issues);

    let Some(role) = message.role.as_ref().and_then(JsonStringField::as_str) else {
        issues.push(MessageStreamIssue::MissingRole(line_number));
        return;
    };
    if !matches!(role, "system" | "user" | "assistant" | "tool") {
        issues.push(MessageStreamIssue::InvalidRole {
            line: line_number,
            role: role.to_owned(),
        });
    }

    let Some(content) = message.content.as_ref() else {
        issues.push(MessageStreamIssue::MissingContent(line_number));
        return;
    };
    if !serde_json::from_value::<MessageContentJson>(content.clone())
        .is_ok_and(|content| content.is_well_formed())
    {
        issues.push(MessageStreamIssue::InvalidContent(line_number));
    }
}

#[derive(Deserialize)]
struct MessageLineJson {
    role: Option<JsonStringField>,
    content: Option<Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MessageContentJson {
    Text(String),
    Parts(Vec<MessageContentPartJson>),
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum MessageContentPartJson {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { path: String },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_call_id: String,
        content: MessageContentJson,
    },
}

impl MessageContentJson {
    fn is_well_formed(&self) -> bool {
        match *self {
            Self::Text(ref text) => {
                let _ = text;
                true
            }
            Self::Parts(ref parts) => parts.iter().all(MessageContentPartJson::is_well_formed),
        }
    }
}

impl MessageContentPartJson {
    fn is_well_formed(&self) -> bool {
        match *self {
            Self::Text { ref text } => {
                let _ = text;
                true
            }
            Self::Image { ref path } => {
                let _ = path;
                true
            }
            Self::ToolResult {
                ref tool_call_id,
                ref content,
            } => {
                let _ = tool_call_id;
                content.is_well_formed()
            }
        }
    }
}

fn append_provider_native_message_field_issues(
    line_number: usize,
    value: &Value,
    issues: &mut Vec<MessageStreamIssue>,
) {
    for field in provider_native_fields(value) {
        issues.push(MessageStreamIssue::ProviderNativeField {
            line: line_number,
            field: field.to_owned(),
        });
    }
}

/// Inspects a stable context JSONL file body.
#[must_use]
pub fn inspect_context_jsonl(kind: ContextJsonlKind, content: &str) -> ContextJsonlReport {
    let mut issues = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        inspect_context_jsonl_line(kind, line_number, line, &mut issues);
    }
    ContextJsonlReport::new(issues)
}

fn inspect_context_jsonl_line(
    kind: ContextJsonlKind,
    line_number: usize,
    line: &str,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    if !line.trim_start().starts_with('{') {
        if serde_json::from_str::<Value>(line).is_ok() {
            issues.push(ContextJsonlIssue::RecordNotObject(line_number));
        } else {
            issues.push(ContextJsonlIssue::InvalidJson(line_number));
        }
        return;
    }
    let Ok(record) = serde_path_to_error::deserialize::<_, ContextJsonlRecordJson>(
        &mut serde_json::Deserializer::from_str(line),
    ) else {
        issues.push(ContextJsonlIssue::InvalidJson(line_number));
        return;
    };

    match kind {
        ContextJsonlKind::Facts => inspect_fact_record(line_number, &record, issues),
        ContextJsonlKind::Decisions => inspect_decision_record(line_number, &record, issues),
        ContextJsonlKind::Refs => inspect_ref_record(line_number, &record, issues),
        ContextJsonlKind::SwapIndex => inspect_swap_index_record(line_number, &record, issues),
        ContextJsonlKind::DedupIndex => inspect_dedup_index_record(line_number, &record, issues),
    }
}

#[derive(Deserialize)]
struct ContextJsonlRecordJson {
    id: Option<JsonStringField>,
    text: Option<JsonStringField>,
    decision: Option<JsonStringField>,
    source: Option<JsonStringField>,
    path: Option<JsonStringField>,
    kind: Option<JsonStringField>,
    summary: Option<JsonStringField>,
    tokens: Option<JsonU64Field>,
    hash: Option<JsonStringField>,
    refs: Option<JsonStringArrayField>,
    bytes: Option<JsonU64Field>,
}

fn inspect_fact_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, record.id.as_ref(), "id", issues, is_context_record_id);
    require_context_string_field(
        line,
        record.text.as_ref(),
        "text",
        issues,
        is_nonempty_single_line,
    );
    require_context_string_field(
        line,
        record.source.as_ref(),
        "source",
        issues,
        is_nonempty_single_line,
    );
}

fn inspect_decision_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, record.id.as_ref(), "id", issues, is_context_record_id);
    require_context_string_field(
        line,
        record.decision.as_ref(),
        "decision",
        issues,
        is_nonempty_single_line,
    );
    require_context_string_field(
        line,
        record.source.as_ref(),
        "source",
        issues,
        is_nonempty_single_line,
    );
}

fn inspect_ref_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, record.id.as_ref(), "id", issues, is_context_record_id);
    require_context_string_field(
        line,
        record.path.as_ref(),
        "path",
        issues,
        is_stable_context_ref_path,
    );
    require_context_string_field(
        line,
        record.kind.as_ref(),
        "kind",
        issues,
        is_context_ref_kind,
    );
    require_context_string_field(
        line,
        record.summary.as_ref(),
        "summary",
        issues,
        is_nonempty_single_line,
    );
}

fn inspect_swap_index_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, record.id.as_ref(), "id", issues, is_context_hash_id);
    require_context_string_field(line, record.kind.as_ref(), "kind", issues, is_swap_kind);
    require_context_string_field(
        line,
        record.source.as_ref(),
        "source",
        issues,
        is_swap_source,
    );
    require_context_string_field(
        line,
        record.summary.as_ref(),
        "summary",
        issues,
        is_nonempty_single_line,
    );
    require_context_number_field(line, record.tokens.as_ref(), "tokens", issues);
}

fn inspect_dedup_index_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(
        line,
        record.hash.as_ref(),
        "hash",
        issues,
        is_context_hash_id,
    );
    require_context_string_array_field(
        line,
        record.refs.as_ref(),
        "refs",
        issues,
        is_nonempty_single_line,
    );
    require_context_number_field(line, record.bytes.as_ref(), "bytes", issues);
    require_context_number_field(line, record.tokens.as_ref(), "tokens", issues);
}

fn require_context_string_field(
    line: usize,
    value: Option<&JsonStringField>,
    field: &str,
    issues: &mut Vec<ContextJsonlIssue>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(value) = value.and_then(JsonStringField::as_str) else {
        issues.push(ContextJsonlIssue::MissingStringField {
            line,
            field: field.to_owned(),
        });
        return;
    };
    if !valid(value) {
        issues.push(ContextJsonlIssue::InvalidField {
            line,
            field: field.to_owned(),
            value: value.to_owned(),
        });
    }
}

fn require_context_string_array_field(
    line: usize,
    values: Option<&JsonStringArrayField>,
    field: &str,
    issues: &mut Vec<ContextJsonlIssue>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(values) = json_string_array_values(values) else {
        issues.push(ContextJsonlIssue::MissingStringArrayField {
            line,
            field: field.to_owned(),
        });
        return;
    };
    if values.is_empty() {
        issues.push(ContextJsonlIssue::MissingStringArrayField {
            line,
            field: field.to_owned(),
        });
        return;
    }
    for value in values {
        if !valid(value) {
            issues.push(ContextJsonlIssue::InvalidField {
                line,
                field: field.to_owned(),
                value: value.clone(),
            });
        }
    }
}

fn require_context_number_field(
    line: usize,
    value: Option<&JsonU64Field>,
    field: &str,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    if !is_json_u64(value) {
        issues.push(ContextJsonlIssue::MissingNumberField {
            line,
            field: field.to_owned(),
        });
    }
}

fn is_json_u64(value: Option<&JsonU64Field>) -> bool {
    value.is_some_and(|value| match *value {
        JsonU64Field::Number(ref number) => {
            let _ = number;
            true
        }
        JsonU64Field::Other(ref value) => {
            let _ = value;
            false
        }
    })
}

fn json_string_array_values(value: Option<&JsonStringArrayField>) -> Option<&[String]> {
    value.and_then(|value| match *value {
        JsonStringArrayField::Strings(ref values) => Some(values.as_slice()),
        JsonStringArrayField::Other(ref value) => {
            let _ = value;
            None
        }
    })
}

fn is_context_record_id(value: &str) -> bool {
    is_object_name(value)
}

fn is_context_hash_id(value: &str) -> bool {
    is_object_name(value)
        && (value.starts_with("sha256-")
            || value.starts_with("sha256_")
            || value.starts_with("sha256."))
}

fn is_nonempty_single_line(value: &str) -> bool {
    !value.is_empty() && !value.contains('\n') && !value.contains('\0')
}

fn is_stable_context_ref_path(value: &str) -> bool {
    is_nonempty_single_line(value)
        && !value.contains('\t')
        && !value.split('/').any(|part| part == "." || part == "..")
}

fn is_context_ref_kind(value: &str) -> bool {
    matches!(
        value,
        "file" | "artifact" | "tool_output" | "swap" | "child_result"
    )
}

fn is_swap_kind(value: &str) -> bool {
    matches!(value, "message_range" | "tool_output" | "file")
}

fn is_swap_source(value: &str) -> bool {
    matches!(value, "messages.jsonl" | "events.jsonl")
        || value.starts_with("context/")
            && validate_context_pack_source(value).is_ok()
            && !value.contains('\0')
}

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
    for field in provider_native_fields(value) {
        issues.push(EventStreamIssue::ProviderNativeField {
            line: line_number,
            field: field.to_owned(),
        });
    }
}

fn provider_native_fields(value: &Value) -> Vec<&str> {
    let mut fields = Vec::new();
    collect_provider_native_fields(value, &mut fields);
    fields
}

fn collect_provider_native_fields<'a>(value: &'a Value, fields: &mut Vec<&'a str>) {
    if let Some(object) = value.as_object() {
        for (key, child) in object {
            if is_provider_native_field(key) {
                fields.push(key);
            }
            collect_provider_native_fields(child, fields);
        }
        return;
    }

    if let Some(items) = value.as_array() {
        for item in items {
            collect_provider_native_fields(item, fields);
        }
    }
}

fn is_provider_native_field(key: &str) -> bool {
    matches!(
        key,
        "thread_id"
            | "response_id"
            | "conversation_id"
            | "provider_thread_id"
            | "provider_response_id"
            | "native_thread"
            | "native_state"
            | "openai_response_id"
            | "anthropic_message_id"
            | "gemini_response_id"
    )
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
