use serde::Deserialize;
use serde_json::Value;

use crate::{
    JsonStringField, JsonU64Field, for_each_jsonl_line, is_object_name,
    validate_context_pack_source,
};

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

impl_issue_report!(ContextJsonlReport, ContextJsonlIssue);

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum JsonStringArrayField {
    Strings(Vec<String>),
    Other(Value),
}

macro_rules! require_context_strings {
    ($line:expr, $record:ident, $issues:expr; $($field:ident => $valid:path),+ $(,)?) => {{
        $(require_context_string_field(
            $line, $record.$field.as_ref(), stringify!($field), $issues, $valid,
        );)+
    }};
}

macro_rules! require_context_string_arrays {
    ($line:expr, $record:ident, $issues:expr; $($field:ident => $valid:path),+ $(,)?) => {{
        $(require_context_string_array_field(
            $line, $record.$field.as_ref(), stringify!($field), $issues, $valid,
        );)+
    }};
}

macro_rules! require_context_numbers {
    ($line:expr, $record:ident, $issues:expr; $($field:ident),+ $(,)?) => {{
        $(require_context_number_field(
            $line, $record.$field.as_ref(), stringify!($field), $issues,
        );)+
    }};
}

#[must_use]
pub fn inspect_context_jsonl(kind: ContextJsonlKind, content: &str) -> ContextJsonlReport {
    let mut issues = Vec::new();
    for_each_jsonl_line(content, |line_number, line| {
        inspect_context_jsonl_line(kind, line_number, line, &mut issues);
    });
    ContextJsonlReport::new(issues)
}

pub(crate) fn inspect_context_jsonl_line(
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
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        issues.push(ContextJsonlIssue::InvalidJson(line_number));
        return;
    };
    if !value.is_object() {
        issues.push(ContextJsonlIssue::RecordNotObject(line_number));
        return;
    }
    let Ok(record) = serde_json::from_str::<ContextJsonlRecordJson>(line) else {
        issues.push(ContextJsonlIssue::InvalidJson(line_number));
        return;
    };

    match kind {
        ContextJsonlKind::Facts => require_context_strings!(line_number, record, issues;
            id => is_object_name, text => is_nonempty_single_line, source => is_nonempty_single_line),
        ContextJsonlKind::Decisions => require_context_strings!(line_number, record, issues;
            id => is_object_name, decision => is_nonempty_single_line, source => is_nonempty_single_line),
        ContextJsonlKind::Refs => require_context_strings!(line_number, record, issues;
            id => is_object_name, path => is_stable_context_ref_path,
            kind => is_context_ref_kind, summary => is_nonempty_single_line),
        ContextJsonlKind::SwapIndex => {
            require_context_strings!(line_number, record, issues;
                id => is_context_hash_id, kind => is_swap_kind, source => is_swap_source,
                summary => is_nonempty_single_line);
            require_context_numbers!(line_number, record, issues; tokens);
        }
        ContextJsonlKind::DedupIndex => {
            require_context_strings!(line_number, record, issues; hash => is_context_hash_id);
            require_context_string_arrays!(line_number, record, issues;
                refs => is_nonempty_single_line);
            require_context_numbers!(line_number, record, issues; bytes, tokens);
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ContextJsonlRecordJson {
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

pub(crate) fn require_context_string_field(
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

pub(crate) fn require_context_string_array_field(
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

pub(crate) fn require_context_number_field(
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

pub(crate) fn json_string_array_values(value: Option<&JsonStringArrayField>) -> Option<&[String]> {
    value.and_then(|value| match *value {
        JsonStringArrayField::Strings(ref values) => Some(values.as_slice()),
        JsonStringArrayField::Other(ref value) => {
            let _ = value;
            None
        }
    })
}

pub(crate) fn is_context_hash_id(value: &str) -> bool {
    is_object_name(value)
        && (value.starts_with("sha256-")
            || value.starts_with("sha256_")
            || value.starts_with("sha256."))
}

pub(crate) fn is_nonempty_single_line(value: &str) -> bool {
    !value.is_empty() && !value.bytes().any(|byte| byte.is_ascii_control())
}

pub(crate) fn is_stable_context_ref_path(value: &str) -> bool {
    is_nonempty_single_line(value) && !value.split('/').any(|part| part == "." || part == "..")
}

pub(crate) fn is_context_ref_kind(value: &str) -> bool {
    matches!(
        value,
        "file" | "artifact" | "tool_output" | "swap" | "child_result"
    )
}

pub(crate) fn is_swap_kind(value: &str) -> bool {
    matches!(value, "message_range" | "tool_output" | "file")
}

pub(crate) fn is_swap_source(value: &str) -> bool {
    matches!(value, "messages.jsonl" | "events.jsonl")
        || value.starts_with("context/")
            && validate_context_pack_source(value).is_ok()
            && !value.contains('\0')
}
