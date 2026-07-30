use super::text::{OpenAiStreamTextEmitter, reject_oversized_stream_tool_call_buffer};
use crate::object::executor::write_model_text_or_tool_call;
use crate::object::runner::openai_chat_tool_call_content;
use serde_json::{Value, json};
use std::io::{self, Write};

#[derive(Default)]
pub(crate) struct OpenAiToolCallStream {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

#[derive(Debug)]
pub(crate) struct OpenAiToolCallDelta {
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) arguments: String,
}

impl OpenAiToolCallStream {
    pub(crate) fn push(&mut self, delta: OpenAiToolCallDelta) {
        if let Some(id) = delta.id {
            self.id = Some(id);
        }
        if let Some(name) = delta.name {
            self.name = Some(name);
        }
        self.arguments.push_str(&delta.arguments);
    }

    pub(crate) fn finish(&mut self) -> io::Result<Option<String>> {
        if self.id.is_none() && self.name.is_none() && self.arguments.is_empty() {
            return Ok(None);
        }
        reject_oversized_stream_tool_call_buffer(&self.arguments)?;
        let Some(name) = self.name.as_deref() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stream tool call missing function name",
            ));
        };
        let Some(id) = self.id.as_deref() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stream tool call missing id",
            ));
        };
        let value = json!({
            "id": id,
            "function": {"name": name, "arguments": self.arguments}
        });
        let Some(tool_call) = openai_chat_tool_call_content(&value) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid stream tool call",
            ));
        };
        *self = Self::default();
        Ok(Some(tool_call))
    }
}

pub(crate) fn emit_openai_stream_tool_call(
    stdout: &mut impl Write,
    emitter: &mut OpenAiStreamTextEmitter<'_>,
    tool_call_stream: &mut OpenAiToolCallStream,
) -> io::Result<bool> {
    let Some(tool_call) = tool_call_stream.finish()? else {
        return Ok(false);
    };
    emitter.finish(stdout)?;
    write_model_text_or_tool_call(stdout, emitter.run, &tool_call)?;
    stdout.flush()?;
    Ok(true)
}

pub(crate) fn openai_stream_tool_call_delta(value: &Value) -> OpenAiToolCallDelta {
    OpenAiToolCallDelta {
        id: value.get("id").and_then(Value::as_str).map(str::to_owned),
        name: value
            .pointer("/function/name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        arguments: value
            .pointer("/function/arguments")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    }
}
