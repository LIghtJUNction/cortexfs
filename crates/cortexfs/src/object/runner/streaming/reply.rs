use super::event::{OpenAiStreamEvent, OpenAiStreamFrame};
use crate::object::runner::{
    openai_response_tool_call_content, text_parts, token_usage_from_value,
};
use crate::provider::openai_response_item_requires_continuation;
use serde_json::Value;

pub(crate) fn openai_responses_stream_event(
    value: &Value,
) -> Result<Option<OpenAiStreamFrame>, String> {
    if value
        .get("item")
        .is_some_and(openai_response_item_requires_continuation)
    {
        return Err("provider response requires host-owned program continuation".to_owned());
    }
    let event = match value.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta" | "response.refusal.delta") => {
            OpenAiStreamEvent::Delta(value_text(value, "delta"))
        }
        Some("response.output_text.done") => {
            OpenAiStreamEvent::FinalText(value_text(value, "text"))
        }
        Some("response.refusal.done") => OpenAiStreamEvent::FinalText(value_text(value, "refusal")),
        Some("response.content_part.done") => OpenAiStreamEvent::FinalText(
            value
                .pointer("/part/text")
                .or_else(|| value.pointer("/part/refusal"))
                .or_else(|| value.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ),
        Some(
            "response.function_call_arguments.delta" | "response.function_call_arguments.done",
        ) => OpenAiStreamEvent::Ignore,
        Some("response.output_item.done") => {
            return Ok(Some(response_output_item_done(value.get("item"))));
        }
        Some("response.completed") => {
            return Ok(Some(response_frame(
                OpenAiStreamEvent::ResponseCompleted(token_usage_from_value(value)),
                false,
            )));
        }
        Some("response.done") => {
            return Ok(Some(response_frame(OpenAiStreamEvent::ResponseDone, false)));
        }
        Some(kind @ ("response.failed" | "response.incomplete" | "error")) => {
            let message = match kind {
                "response.failed" => value
                    .pointer("/response/error/message")
                    .or_else(|| value.pointer("/error/message")),
                "response.incomplete" => value
                    .pointer("/response/incomplete_details/reason")
                    .or_else(|| value.pointer("/error/message")),
                _ => value.pointer("/error/message"),
            };
            return Err(message
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("provider stream failed")
                .to_owned());
        }
        _ => return Ok(None),
    };
    Ok(Some(response_frame(event, false)))
}

fn response_output_item_done(item: Option<&Value>) -> OpenAiStreamFrame {
    let event = item
        .and_then(openai_response_tool_call_content)
        .map_or_else(
            || OpenAiStreamEvent::FinalText(response_output_item_text(item)),
            OpenAiStreamEvent::ToolCall,
        );
    response_frame(event, false)
}

fn response_frame(event: OpenAiStreamEvent, terminal: bool) -> OpenAiStreamFrame {
    OpenAiStreamFrame {
        event,
        terminal,
        chat_terminal: false,
    }
}

fn value_text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

pub(crate) fn response_output_item_text(item: Option<&Value>) -> String {
    let Some(item) = item else {
        return String::new();
    };
    if let Some(text) = item.get("output_text").and_then(Value::as_str) {
        return text.to_owned();
    }
    item.get("content")
        .and_then(Value::as_array)
        .map(|parts| text_parts(parts.iter()))
        .unwrap_or_default()
}
