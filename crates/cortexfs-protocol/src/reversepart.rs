use crate::gemini::Content as GeminiContent;
use crate::openaichat::{Content as OpenAiContent, Function, ImageUrl, Message, Part, ToolCall};
use std::borrow::Cow;

pub(super) fn gemini_message<'a>(content: &GeminiContent<'a>) -> Message<'a> {
    let mut role = content.role.clone().unwrap_or(Cow::Borrowed("user"));
    if role == "model" {
        role = Cow::Borrowed("assistant");
    }
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
                id: call.id.as_ref().unwrap_or(&call.name).clone(),
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
            let id = response.id.as_ref().unwrap_or(&response.name).clone();
            result = Some((Cow::clone(&response.name), id, response.response.get()));
        }
    }
    if let Some((name, id, value)) = result {
        return Message {
            role: Cow::Borrowed("tool"),
            content: Some(OpenAiContent::Text(Cow::Borrowed(value))),
            name: Some(name),
            tool_call_id: Some(id),
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
