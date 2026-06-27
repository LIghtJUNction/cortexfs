use serde::Deserialize;
use serde_json::Value;

use crate::{JsonStringField, provider_native_fields};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageStreamIssue {
    InvalidJson(usize),
    MessageNotObject(usize),
    MissingRole(usize),
    InvalidRole { line: usize, role: String },
    MissingContent(usize),
    InvalidContent(usize),
    ProviderNativeField { line: usize, field: String },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MessageStreamReport {
    issues: Vec<MessageStreamIssue>,
}

impl_issue_report!(MessageStreamReport, MessageStreamIssue);

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
    if !content.is_object() && !content.is_string() && !content.is_array() {
        issues.push(MessageStreamIssue::InvalidContent(line_number));
        return;
    }
    if !serde_path_to_error::deserialize::<_, MessageContentLineJson>(
        &mut serde_json::Deserializer::from_str(line),
    )
    .is_ok_and(|line| {
        line.content
            .as_ref()
            .is_some_and(MessageContentJson::is_well_formed)
    }) {
        issues.push(MessageStreamIssue::InvalidContent(line_number));
    }
}

#[derive(Deserialize)]
struct MessageLineJson {
    role: Option<JsonStringField>,
    content: Option<Value>,
}

#[derive(Deserialize)]
struct MessageContentLineJson {
    content: Option<MessageContentJson>,
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
