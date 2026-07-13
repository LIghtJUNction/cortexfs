use serde_json::json;

use super::*;
use serde_json::Value;
use std::io;

pub(crate) struct StreamFailure {
    pub(crate) message: String,
    pub(crate) can_fallback: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OpenAiStreamApi {
    Chat,
    Responses,
}
pub(crate) fn call_openai_chat_streaming(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let target = chat_completions_target(transport);
    let body = openai_chat_body(request.model, request.input, true, request.effort);
    call_openai_sse_streaming(
        &target,
        request.api_key,
        &body,
        OpenAiStreamApi::Chat,
        run,
        stdout,
    )
}
pub(crate) fn call_openai_responses_streaming(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let target = responses_target(transport);
    let body = openai_responses_body(request.model, request.input, true, request.effort);
    call_openai_sse_streaming(
        &target,
        request.api_key,
        &body,
        OpenAiStreamApi::Responses,
        run,
        stdout,
    )
}
#[expect(
    clippy::too_many_lines,
    reason = "streaming provider state machine keeps I/O cleanup paths local"
)]
pub(crate) fn call_openai_sse_streaming(
    target: &CurlJsonTarget,
    api_key: Option<&str>,
    body: &str,
    api: OpenAiStreamApi,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let mut child = start_curl_json(target, api_key, body).map_err(|message| StreamFailure {
        message,
        can_fallback: true,
    })?;
    let (child_stdout, stderr_reader) = provider_stream_pipes(&mut child)?;
    let mut text_emitter = OpenAiStreamTextEmitter::new(run);
    let mut tool_call_stream = OpenAiToolCallStream::default();
    let mut emitted = false;
    let mut done = false;
    let mut stream = BufReader::new(child_stdout);
    loop {
        let line = match read_provider_stream_line(&mut stream) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => {
                cleanup_curl_child(&mut child);
                return Err(StreamFailure {
                    message: format!("cannot read provider stream: {error}"),
                    can_fallback: !emitted,
                });
            }
        };
        let frame = match openai_stream_event(&line) {
            Ok(frame) => frame,
            Err(message) => {
                cleanup_curl_child(&mut child);
                return Err(StreamFailure {
                    message,
                    can_fallback: !emitted,
                });
            }
        };
        let terminal = frame.terminal && (!frame.chat_terminal || api == OpenAiStreamApi::Chat);
        match frame.event {
            OpenAiStreamEvent::Delta(text) if !text.is_empty() => {
                if let Err(error) = text_emitter
                    .push(stdout, &text)
                    .and_then(|()| stdout.flush())
                {
                    cleanup_curl_child(&mut child);
                    return Err(StreamFailure {
                        message: format!("cannot write output: {error}"),
                        can_fallback: false,
                    });
                }
                emitted = true;
            }
            OpenAiStreamEvent::Delta(_empty) => {}
            OpenAiStreamEvent::FinalText(text) if !emitted && !text.is_empty() => {
                if let Err(error) = text_emitter
                    .push(stdout, &text)
                    .and_then(|()| stdout.flush())
                {
                    cleanup_curl_child(&mut child);
                    return Err(StreamFailure {
                        message: format!("cannot write output: {error}"),
                        can_fallback: false,
                    });
                }
                emitted = true;
            }
            OpenAiStreamEvent::FinalText(_text) => {}
            OpenAiStreamEvent::Usage(usage) => {
                if let Err(error) =
                    write_model_usage(stdout, run, usage).and_then(|()| stdout.flush())
                {
                    cleanup_curl_child(&mut child);
                    return Err(StreamFailure {
                        message: format!("cannot write output: {error}"),
                        can_fallback: false,
                    });
                }
            }
            OpenAiStreamEvent::ToolCallDelta(delta) => {
                tool_call_stream.push(delta);
            }
            OpenAiStreamEvent::ToolCall(tool_call) => {
                if let Err(error) = text_emitter
                    .finish(stdout)
                    .and_then(|()| write_model_text_or_tool_call(stdout, run, &tool_call))
                    .and_then(|()| stdout.flush())
                {
                    cleanup_curl_child(&mut child);
                    return Err(StreamFailure {
                        message: format!("cannot write output: {error}"),
                        can_fallback: false,
                    });
                }
                emitted = true;
            }
            OpenAiStreamEvent::ToolCallsDone
            | OpenAiStreamEvent::Done
            | OpenAiStreamEvent::Ignore => {}
        }
        if terminal {
            match emit_openai_stream_tool_call(
                stdout,
                run,
                &mut text_emitter,
                &mut tool_call_stream,
            ) {
                Ok(true) => emitted = true,
                Ok(false) => {}
                Err(error) => {
                    cleanup_curl_child(&mut child);
                    return Err(StreamFailure {
                        message: format!("cannot write output: {error}"),
                        can_fallback: false,
                    });
                }
            }
            done = true;
        }
    }
    let status = child.wait().map_err(|error| StreamFailure {
        message: format!("cannot run curl: {error}"),
        can_fallback: !emitted,
    })?;
    if !status.success() {
        let stderr = collect_child_stderr(stderr_reader);
        let message = provider_stream_failure_message(status, &stderr);
        if emitted {
            text_emitter
                .finish(stdout)
                .and_then(|()| stdout.flush())
                .map_err(|error| StreamFailure {
                    message: format!("cannot write output: {error}"),
                    can_fallback: false,
                })?;
            return Err(StreamFailure {
                can_fallback: false,
                message,
            });
        }
        return Err(StreamFailure {
            message,
            can_fallback: stderr.contains("Operation timed out") || !emitted,
        });
    }
    match emit_openai_stream_tool_call(stdout, run, &mut text_emitter, &mut tool_call_stream) {
        Ok(true) => emitted = true,
        Ok(false) => {}
        Err(error) => {
            return Err(StreamFailure {
                message: format!("cannot write output: {error}"),
                can_fallback: false,
            });
        }
    }
    if let Err(error) = text_emitter.finish(stdout).and_then(|()| stdout.flush()) {
        return Err(StreamFailure {
            message: format!("cannot write output: {error}"),
            can_fallback: false,
        });
    }
    if emitted && done {
        Ok(())
    } else {
        Err(StreamFailure {
            message: if done {
                "provider stream produced no answer text".to_owned()
            } else if emitted {
                "provider stream ended without a terminal event".to_owned()
            } else {
                "provider stream produced no content".to_owned()
            },
            can_fallback: !emitted,
        })
    }
}
pub(crate) fn provider_stream_pipes(
    child: &mut Child,
) -> Result<
    (
        std::process::ChildStdout,
        Option<thread::JoinHandle<String>>,
    ),
    StreamFailure,
> {
    let Some(child_stdout) = child.stdout.take() else {
        cleanup_curl_child(child);
        return Err(StreamFailure {
            message: "cannot read provider stream".to_owned(),
            can_fallback: true,
        });
    };
    Ok((
        child_stdout,
        child.stderr.take().map(spawn_child_stderr_reader),
    ))
}
pub(crate) fn read_provider_stream_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_PROVIDER_STREAM_LINE_BYTES.saturating_add(1))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let read = reader.take(limit).read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > MAX_PROVIDER_STREAM_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "provider stream line exceeds byte limit",
        ));
    }
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
#[derive(Default)]
pub(crate) struct OpenAiToolCallStream {
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) arguments: String,
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
        let value = json!({
            "id": self.id.as_deref().unwrap_or("call-1"),
            "function": {
                "name": name,
                "arguments": self.arguments
            }
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
    run: &str,
    text_emitter: &mut OpenAiStreamTextEmitter<'_>,
    tool_call_stream: &mut OpenAiToolCallStream,
) -> io::Result<bool> {
    let Some(tool_call) = tool_call_stream.finish()? else {
        return Ok(false);
    };
    text_emitter.finish(stdout)?;
    write_model_text_or_tool_call(stdout, run, &tool_call)?;
    stdout.flush()?;
    Ok(true)
}
pub(crate) enum StreamTextMode {
    Undecided,
    BufferToolCall,
    Plain,
}
pub(crate) struct OpenAiStreamTextEmitter<'a> {
    pub(crate) run: &'a str,
    pub(crate) mode: StreamTextMode,
    pub(crate) buffer: String,
}
impl<'a> OpenAiStreamTextEmitter<'a> {
    pub(crate) fn new(run: &'a str) -> Self {
        Self {
            run,
            mode: StreamTextMode::Undecided,
            buffer: String::new(),
        }
    }
    pub(crate) fn push(&mut self, stdout: &mut impl Write, text: &str) -> io::Result<()> {
        match self.mode {
            StreamTextMode::Plain => write_model_delta(stdout, self.run, text),
            StreamTextMode::BufferToolCall => {
                self.buffer.push_str(text);
                reject_oversized_stream_tool_call_buffer(&self.buffer)?;
                Ok(())
            }
            StreamTextMode::Undecided => {
                self.buffer.push_str(text);
                reject_oversized_stream_tool_call_buffer(&self.buffer)?;
                let trimmed = self.buffer.trim_start();
                if trimmed.is_empty() {
                    return Ok(());
                }
                if trimmed.starts_with('{') {
                    self.mode = StreamTextMode::BufferToolCall;
                    return Ok(());
                }
                self.mode = StreamTextMode::Plain;
                let buffered = std::mem::take(&mut self.buffer);
                write_model_delta(stdout, self.run, &buffered)
            }
        }
    }
    pub(crate) fn finish(&mut self, stdout: &mut impl Write) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let buffered = std::mem::take(&mut self.buffer);
        write_model_text_or_tool_call(stdout, self.run, &buffered)
    }
}
pub(crate) fn reject_oversized_stream_tool_call_buffer(buffer: &str) -> io::Result<()> {
    if buffer.len() > MAX_STREAM_TOOL_CALL_BUFFER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("stream tool call buffer exceeds {MAX_STREAM_TOOL_CALL_BUFFER_BYTES} bytes"),
        ));
    }
    Ok(())
}
pub(crate) fn provider_stream_failure_message(
    status: std::process::ExitStatus,
    stderr: &str,
) -> String {
    if stderr.trim().is_empty() {
        format!("provider stream request failed with {status}")
    } else {
        format!(
            "provider stream request failed with {status}: {}",
            stderr.trim()
        )
    }
}
pub(crate) enum OpenAiStreamEvent {
    Delta(String),
    FinalText(String),
    Usage(TokenUsage),
    ToolCallDelta(OpenAiToolCallDelta),
    ToolCallsDone,
    ToolCall(String),
    Done,
    Ignore,
}
pub(crate) struct OpenAiStreamFrame {
    pub(crate) event: OpenAiStreamEvent,
    pub(crate) terminal: bool,
    pub(crate) chat_terminal: bool,
}
pub(crate) fn openai_stream_event(line: &str) -> Result<OpenAiStreamFrame, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || !line.starts_with("data:") {
        return Ok(OpenAiStreamFrame {
            event: OpenAiStreamEvent::Ignore,
            terminal: false,
            chat_terminal: false,
        });
    }
    let data = line.trim_start_matches("data:").trim();
    if data == "[DONE]" {
        return Ok(OpenAiStreamFrame {
            event: OpenAiStreamEvent::Done,
            terminal: true,
            chat_terminal: false,
        });
    }
    let value = serde_json::from_str::<Value>(data)
        .map_err(|error| format!("invalid provider stream json: {error}"))?;
    if let Some(frame) = openai_responses_stream_event(&value)? {
        return Ok(frame);
    }
    let finish_reason = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .filter(|finish_reason| !finish_reason.trim().is_empty());
    let terminal = finish_reason.is_some();
    if let Some(usage) = token_usage_from_value(&value) {
        return Ok(OpenAiStreamFrame {
            event: OpenAiStreamEvent::Usage(usage),
            terminal,
            chat_terminal: terminal,
        });
    }
    if let Some(tool_call) = value.pointer("/choices/0/delta/tool_calls/0") {
        return Ok(OpenAiStreamFrame {
            event: OpenAiStreamEvent::ToolCallDelta(openai_stream_tool_call_delta(tool_call)),
            terminal,
            chat_terminal: terminal,
        });
    }
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("delta").and_then(Value::as_str))
        .or_else(|| value.get("output_text").and_then(Value::as_str))
        .unwrap_or_default();
    Ok(OpenAiStreamFrame {
        event: if text.is_empty() && terminal {
            if finish_reason == Some("tool_calls") {
                OpenAiStreamEvent::ToolCallsDone
            } else {
                OpenAiStreamEvent::Done
            }
        } else {
            OpenAiStreamEvent::Delta(text.to_owned())
        },
        terminal,
        chat_terminal: terminal,
    })
}

fn openai_responses_stream_event(value: &Value) -> Result<Option<OpenAiStreamFrame>, String> {
    match value.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta") => {
            return Ok(Some(OpenAiStreamFrame {
                event: OpenAiStreamEvent::Delta(
                    value
                        .get("delta")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
                terminal: false,
                chat_terminal: false,
            }));
        }
        Some("response.output_text.done") => {
            return Ok(Some(OpenAiStreamFrame {
                event: OpenAiStreamEvent::FinalText(
                    value
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
                terminal: false,
                chat_terminal: false,
            }));
        }
        Some("response.content_part.done") => {
            return Ok(Some(OpenAiStreamFrame {
                event: OpenAiStreamEvent::FinalText(
                    value
                        .pointer("/part/text")
                        .or_else(|| value.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
                terminal: false,
                chat_terminal: false,
            }));
        }
        Some(
            "response.function_call_arguments.delta" | "response.function_call_arguments.done",
        ) => {
            return Ok(Some(OpenAiStreamFrame {
                event: OpenAiStreamEvent::Ignore,
                terminal: false,
                chat_terminal: false,
            }));
        }
        Some("response.output_item.done") => {
            if let Some(tool_call) = response_output_item_tool_call(value.get("item")) {
                return Ok(Some(OpenAiStreamFrame {
                    event: OpenAiStreamEvent::ToolCall(tool_call),
                    terminal: false,
                    chat_terminal: false,
                }));
            }
            return Ok(Some(OpenAiStreamFrame {
                event: OpenAiStreamEvent::FinalText(response_output_item_text(value.get("item"))),
                terminal: false,
                chat_terminal: false,
            }));
        }
        Some("response.completed" | "response.done") => {
            return Ok(Some(OpenAiStreamFrame {
                event: OpenAiStreamEvent::Done,
                terminal: true,
                chat_terminal: false,
            }));
        }
        Some("response.failed" | "response.incomplete" | "error") => {
            let message = value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("provider stream failed");
            return Err(message.to_owned());
        }
        _ => {}
    }
    Ok(None)
}
pub(crate) fn response_output_item_tool_call(item: Option<&Value>) -> Option<String> {
    openai_response_tool_call_content(item?)
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
pub(crate) fn response_output_item_text(item: Option<&Value>) -> String {
    let Some(item) = item else {
        return String::new();
    };
    if let Some(text) = item.get("output_text").and_then(Value::as_str) {
        return text.to_owned();
    }
    let Some(parts) = item.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    let mut output = String::new();
    for part in parts {
        if matches!(
            part.get("type").and_then(Value::as_str),
            Some("output_text" | "text")
        ) && let Some(text) = part.get("text").and_then(Value::as_str)
        {
            output.push_str(text);
        }
    }
    output
}
#[cfg(test)]
mod streaming_tests {
    use super::*;
    #[test]
    fn responses_stream_function_call_done_becomes_tool_call_event() -> Result<(), String> {
        let line = concat!(
            "data: ",
            r#"{"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_123","name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}"#
        );
        let OpenAiStreamEvent::ToolCall(frame) = openai_stream_event(line)?.event else {
            return Err("expected tool call event".to_owned());
        };
        let value = serde_json::from_str::<Value>(&frame).map_err(|error| error.to_string())?;
        assert_eq!(value.get("type"), Some(&json!("tool_call")));
        assert_eq!(value.get("id"), Some(&json!("call_123")));
        assert_eq!(value.get("name"), Some(&json!("tsh")));
        assert_eq!(value.pointer("/arguments/args/0"), Some(&json!("tools")));
        Ok(())
    }
    #[test]
    fn responses_stream_function_call_argument_delta_is_not_text() -> Result<(), String> {
        let line = concat!(
            "data: ",
            r#"{"type":"response.function_call_arguments.delta","delta":"{\"args\":[\"tools\"]}"}"#
        );
        let event = openai_stream_event(line)?.event;
        assert!(matches!(event, OpenAiStreamEvent::Ignore));
        Ok(())
    }
}
