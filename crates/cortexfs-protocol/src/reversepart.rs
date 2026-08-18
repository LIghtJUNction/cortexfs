use crate::gemini::Content as GeminiContent;
use crate::openaichat::{Content as OpenAiContent, Function, ImageUrl, Message, Part, ToolCall};
use std::borrow::Cow;

pub(super) fn gemini_message<'a>(content: &GeminiContent<'a>) -> Message<'a> {
    let role = content.role.as_ref().map_or_else(
        || Cow::Borrowed("user"),
        |value| {
            if value.as_ref() == "model" {
                Cow::Borrowed("assistant")
            } else {
                Cow::clone(value)
            }
        },
    );
    let mut text = Vec::new();
    let mut parts = Vec::new();
    let mut calls = Vec::new();
    let mut result = None;
    for part in &content.parts {
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
                id: Cow::clone(&call.name),
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
            result = Some((Cow::clone(&response.name), response.response.get()));
        }
    }
    if let Some((name, value)) = result {
        return Message {
            role: Cow::Borrowed("tool"),
            content: Some(OpenAiContent::Text(Cow::Borrowed(value))),
            name: Some(name.clone()),
            tool_call_id: Some(name),
            tool_calls: Vec::new(),
        };
    }
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
