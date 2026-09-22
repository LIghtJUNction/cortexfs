use crate::gemini::Content as GeminiContent;
use crate::openaichat::{Content as OpenAiContent, Function, ImageUrl, Message, Part, ToolCall};
use std::borrow::Cow;

pub(super) fn gemini_messages<'a>(content: &GeminiContent<'a>) -> Vec<Message<'a>> {
    let mut role = content.role.clone().unwrap_or(Cow::Borrowed("user"));
    if role == "model" {
        role = Cow::Borrowed("assistant");
    }
    let mut text = Vec::new();
    let mut parts = Vec::new();
    let mut calls = Vec::new();
    let mut results = Vec::new();
    for (index, part) in content.parts.iter().enumerate() {
        if let Some(value) = part.text.as_ref() {
            text.push(Cow::clone(value));
        }
        if let Some(file) = part.file_data.as_ref() {
            parts.push(Part {
                kind: Cow::Borrowed("image_url"),
                text: None,
                image_url: Some(ImageUrl {
                    url: Cow::clone(&file.file_uri),
                    detail: None,
                }),
            });
        }
        if let Some(call) = part.function_call.as_ref() {
            calls.push(ToolCall {
                id: Cow::Owned(crate::gemini::correlation_id(call.id.as_deref(), index)),
                kind: Cow::Borrowed("function"),
                function: Function {
                    name: Cow::clone(&call.name),
                    description: None,
                    parameters: None,
                    arguments: Some(Cow::Borrowed(call.args.get())),
                },
            });
        }
        if let Some(response) = part.function_response.as_ref() {
            results.push(Message {
                role: Cow::Borrowed("tool"),
                content: Some(OpenAiContent::Text(Cow::Borrowed(response.response.get()))),
                name: Some(Cow::clone(&response.name)),
                tool_call_id: Some(Cow::Owned(crate::gemini::correlation_id(
                    response.id.as_deref(),
                    index,
                ))),
                tool_calls: Vec::new(),
            });
        }
    }
    if results.is_empty() || !text.is_empty() || !parts.is_empty() || !calls.is_empty() {
        results.insert(0, message(role, text, parts, calls));
    }
    results
}

fn message<'a>(
    role: Cow<'a, str>,
    text: Vec<Cow<'a, str>>,
    mut parts: Vec<Part<'a>>,
    calls: Vec<ToolCall<'a>>,
) -> Message<'a> {
    let content = if !parts.is_empty() || text.len() > 1 {
        parts.extend(text.into_iter().map(|value| Part {
            kind: Cow::Borrowed("text"),
            text: Some(value),
            image_url: None,
        }));
        Some(OpenAiContent::Parts(parts))
    } else {
        Some(OpenAiContent::Text(
            text.into_iter().next().unwrap_or(Cow::Borrowed("")),
        ))
    };
    Message {
        role,
        content,
        name: None,
        tool_call_id: None,
        tool_calls: calls,
    }
}
