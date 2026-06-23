use serde::Deserialize;
use serde_json::Value;

use crate::{JsonStringField, JsonU64Field, is_object_name, validate_context_pack_source};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextJsonlKind {
    Facts,
    Decisions,
    Refs,
    SwapIndex,
    DedupIndex,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextJsonlIssue {
    InvalidJson(usize),
    RecordNotObject(usize),
    MissingStringField {
        line: usize,
        field: String,
    },
    MissingNumberField {
        line: usize,
        field: String,
    },
    MissingStringArrayField {
        line: usize,
        field: String,
    },
    InvalidField {
        line: usize,
        field: String,
        value: String,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextJsonlReport {
    issues: Vec<ContextJsonlIssue>,
}

impl ContextJsonlReport {
    #[must_use]
    pub const fn new(issues: Vec<ContextJsonlIssue>) -> Self {
        Self { issues }
    }

    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    #[must_use]
    pub fn issues(&self) -> &[ContextJsonlIssue] {
        &self.issues
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonStringArrayField {
    Strings(Vec<String>),
    Other(Value),
}

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
    inspect_text_source_record(line, record, "text", record.text.as_ref(), issues);
}

fn inspect_decision_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    inspect_text_source_record(line, record, "decision", record.decision.as_ref(), issues);
}

fn inspect_text_source_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    text_field: &str,
    text_value: Option<&JsonStringField>,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, record.id.as_ref(), "id", issues, is_context_record_id);
    require_context_string_field(
        line,
        text_value,
        text_field,
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
    if !crate::is_json_u64(value) {
        issues.push(ContextJsonlIssue::MissingNumberField {
            line,
            field: field.to_owned(),
        });
    }
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
