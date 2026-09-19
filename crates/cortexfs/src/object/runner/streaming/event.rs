use super::reply::openai_responses_stream_event;
use super::tool::{OpenAiToolCallDelta, openai_stream_tool_call_delta};
use crate::object::runner::{TokenUsage, token_usage_from_value};
use serde_json::Value;

pub(crate) enum OpenAiStreamEvent {
    Delta(String),
    FinalText(String),
    Usage(TokenUsage),
    ToolCallDelta(OpenAiToolCallDelta),
    ToolCall(String),
    ResponseCompleted(Option<TokenUsage>),
    ResponseDone,
    Done,
    Ignore,
}

pub(crate) struct OpenAiStreamFrame {
    pub(crate) event: OpenAiStreamEvent,
    pub(crate) terminal: bool,
    pub(crate) chat_terminal: bool,
}

pub(crate) fn openai_stream_event(line: &str) -> Result<OpenAiStreamFrame, String> {
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(chat_frame(OpenAiStreamEvent::Ignore, false));
    };
    let data = data.strip_prefix(' ').unwrap_or(data);
    if data == "[DONE]" {
        return Ok(chat_frame(OpenAiStreamEvent::Done, true));
    }
    let value = serde_json::from_str::<Value>(data)
        .map_err(|error| format!("invalid provider stream json: {error}"))?;
    if let Some(frame) = openai_responses_stream_event(&value)? {
        return Ok(frame);
    }
    let finish_reason = crate::object::runner::openai_chat_finish_reason(&value)?;
    let terminal = finish_reason.is_some();
    if let Some(usage) = token_usage_from_value(&value) {
        return Ok(chat_frame(OpenAiStreamEvent::Usage(usage), terminal));
    }
    if value.pointer("/choices/0/delta/tool_calls/1").is_some() {
        return Err("multi-call".to_owned());
    }
    if let Some(tool_call) = value.pointer("/choices/0/delta/tool_calls/0") {
        return Ok(chat_frame(
            OpenAiStreamEvent::ToolCallDelta(openai_stream_tool_call_delta(tool_call)?),
            terminal,
        ));
    }
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("delta").and_then(Value::as_str))
        .or_else(|| value.get("output_text").and_then(Value::as_str))
        .unwrap_or_default();
    let event = if text.is_empty() && terminal {
        OpenAiStreamEvent::Done
    } else {
        OpenAiStreamEvent::Delta(text.to_owned())
    };
    Ok(chat_frame(event, terminal))
}

fn chat_frame(event: OpenAiStreamEvent, terminal: bool) -> OpenAiStreamFrame {
    OpenAiStreamFrame {
        event,
        terminal,
        chat_terminal: terminal,
    }
}
