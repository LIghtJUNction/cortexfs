use super::text::{OpenAiStreamTextEmitter, reject_oversized_stream_tool_call_buffer};
use crate::object::executor::write_model_text_or_tool_call;
use crate::object::runner::openai_chat_tool_call_content;
use serde_json::{Value, json};
use std::io::{self, Write};

#[derive(Default)]
pub(crate) struct OpenAiToolCallStream {
    index: Option<usize>,
    multiple: bool,
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub(crate) struct OpenAiToolCallDelta {
    pub(crate) index: Option<usize>,
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) arguments: String,
}

impl OpenAiToolCallStream {
    pub(crate) fn push(&mut self, delta: OpenAiToolCallDelta) {
        if let Some(index) = delta.index {
            if self.index.is_some_and(|active| active != index) {
                self.multiple = true;
                return;
            }
            self.index = Some(index);
        }
        self.id = delta.id.filter(|id| !id.is_empty()).or_else(|| self.id.take());
        self.name = delta.name.filter(|name| !name.is_empty()).or_else(|| self.name.take());
        self.arguments.push_str(&delta.arguments);
    }

    pub(crate) fn finish(&mut self) -> io::Result<Option<String>> {
        if self.id.is_none() && self.name.is_none() && self.arguments.is_empty() {
            return Ok(None);
        }
        reject_oversized_stream_tool_call_buffer(&self.arguments)?;
        let name = self
            .name
            .as_deref()
            .ok_or_else(|| invalid("stream tool call missing function name"))?;
        let id = self.id.as_deref().ok_or_else(|| invalid("stream tool call missing id"))?;
        let value = json!({"id": id, "function": {"name": name, "arguments": self.arguments}});
        let tool_call = openai_chat_tool_call_content(&value)
            .ok_or_else(|| invalid("invalid stream tool call"))?;
        *self = Self::default();
        Ok(Some(tool_call))
    }
}

pub(crate) fn emit_openai_stream_tool_call(
    stdout: &mut impl Write,
    emitter: &mut OpenAiStreamTextEmitter<'_>,
    tool_call_stream: &mut OpenAiToolCallStream,
) -> io::Result<bool> {
    if tool_call_stream.multiple {
        return Err(invalid("multi-call"));
    }
    let Some(tool_call) = tool_call_stream.finish()? else {
        return Ok(false);
    };
    emitter.finish(stdout)?;
    write_model_text_or_tool_call(stdout, emitter.run, &tool_call)?;
    stdout.flush()?;
    Ok(true)
}

pub(crate) fn openai_stream_tool_call_delta(value: &Value) -> Result<OpenAiToolCallDelta, String> {
    let string = |path| match value.pointer(path) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.to_owned())),
        Some(_) => Err(format!("invalid stream tool call {path}")),
    };
    let index = value
        .get("index")
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| "invalid stream tool call index".to_owned())
        })
        .transpose()?;
    Ok(OpenAiToolCallDelta {
        index,
        id: string("/id")?,
        name: string("/function/name")?,
        arguments: string("/function/arguments")?.unwrap_or_default(),
    })
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_delta_validates_field_types() {
        for value in [json!({"id": 1}), json!({"index": "0"})] {
            assert!(openai_stream_tool_call_delta(&value).is_err());
        }
    }
}
