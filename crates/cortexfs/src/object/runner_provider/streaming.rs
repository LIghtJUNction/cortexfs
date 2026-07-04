struct StreamFailure {
    message: String,
    can_fallback: bool,
}

fn call_openai_chat_streaming(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let target = chat_completions_target(transport);
    let body = openai_chat_body(request.model, request.input, true, request.effort);
    call_openai_sse_streaming(&target, request.api_key, &body, run, stdout)
}

fn call_openai_responses_streaming(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let target = responses_target(transport);
    let body = openai_responses_body(request.model, request.input, true, request.effort);
    call_openai_sse_streaming(&target, request.api_key, &body, run, stdout)
}

fn call_openai_sse_streaming(
    target: &CurlJsonTarget,
    api_key: Option<&str>,
    body: &str,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let mut child = start_curl_json(target, api_key, body).map_err(|message| StreamFailure {
        message,
        can_fallback: true,
    })?;
    let (child_stdout, stderr_reader) = provider_stream_pipes(&mut child)?;
    let mut text_emitter = OpenAiStreamTextEmitter::new(run);
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
        match openai_stream_event(&line) {
            Ok(OpenAiStreamEvent::Delta(text)) if !text.is_empty() => {
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
            Ok(OpenAiStreamEvent::Delta(_empty)) => {}
            Ok(OpenAiStreamEvent::FinalText(text)) if !emitted && !text.is_empty() => {
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
            Ok(OpenAiStreamEvent::FinalText(_text)) => {}
            Ok(OpenAiStreamEvent::Usage(usage)) => {
                if let Err(error) = write_model_usage(stdout, run, usage)
                    .and_then(|()| stdout.flush())
                {
                    cleanup_curl_child(&mut child);
                    return Err(StreamFailure {
                        message: format!("cannot write output: {error}"),
                        can_fallback: false,
                    });
                }
            }
            Ok(OpenAiStreamEvent::Done) => done = true,
            Ok(OpenAiStreamEvent::Ignore) => {}
            Err(message) => {
                cleanup_curl_child(&mut child);
                return Err(StreamFailure {
                    message,
                    can_fallback: !emitted,
                });
            }
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
                can_fallback: stderr.contains("Operation timed out"),
                message,
            });
        }
        return Err(StreamFailure {
            message,
            can_fallback: stderr.contains("Operation timed out") || !emitted,
        });
    }
    if let Err(error) = text_emitter.finish(stdout).and_then(|()| stdout.flush()) {
        return Err(StreamFailure {
            message: format!("cannot write output: {error}"),
            can_fallback: false,
        });
    }
    if emitted {
        Ok(())
    } else {
        Err(StreamFailure {
            message: if done {
                "provider stream produced no answer text".to_owned()
            } else {
                "provider stream produced no content".to_owned()
            },
            can_fallback: true,
        })
    }
}

fn provider_stream_pipes(
    child: &mut Child,
) -> Result<(std::process::ChildStdout, Option<thread::JoinHandle<String>>), StreamFailure> {
    let Some(child_stdout) = child.stdout.take() else {
        cleanup_curl_child(child);
        return Err(StreamFailure {
            message: "cannot read provider stream".to_owned(),
            can_fallback: true,
        });
    };
    Ok((child_stdout, child.stderr.take().map(spawn_child_stderr_reader)))
}

fn read_provider_stream_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
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

enum StreamTextMode {
    Undecided,
    BufferToolCall,
    Plain,
}

struct OpenAiStreamTextEmitter<'a> {
    run: &'a str,
    mode: StreamTextMode,
    buffer: String,
}

impl<'a> OpenAiStreamTextEmitter<'a> {
    fn new(run: &'a str) -> Self {
        Self {
            run,
            mode: StreamTextMode::Undecided,
            buffer: String::new(),
        }
    }

    fn push(&mut self, stdout: &mut impl Write, text: &str) -> io::Result<()> {
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

    fn finish(&mut self, stdout: &mut impl Write) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let buffered = std::mem::take(&mut self.buffer);
        write_model_text_or_tool_call(stdout, self.run, &buffered)
    }
}
fn reject_oversized_stream_tool_call_buffer(buffer: &str) -> io::Result<()> {
    if buffer.len() > MAX_STREAM_TOOL_CALL_BUFFER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "stream tool call buffer exceeds {MAX_STREAM_TOOL_CALL_BUFFER_BYTES} bytes"
            ),
        ));
    }
    Ok(())
}

fn provider_stream_failure_message(status: std::process::ExitStatus, stderr: &str) -> String {
    if stderr.trim().is_empty() {
        format!("provider stream request failed with {status}")
    } else {
        format!("provider stream request failed with {status}: {}", stderr.trim())
    }
}
enum OpenAiStreamEvent {
    Delta(String),
    FinalText(String),
    Usage(TokenUsage),
    Done,
    Ignore,
}

fn openai_stream_event(line: &str) -> Result<OpenAiStreamEvent, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || !line.starts_with("data:") {
        return Ok(OpenAiStreamEvent::Ignore);
    }
    let data = line.trim_start_matches("data:").trim();
    if data == "[DONE]" {
        return Ok(OpenAiStreamEvent::Done);
    }
    let value = serde_json::from_str::<Value>(data)
        .map_err(|error| format!("invalid provider stream json: {error}"))?;
    match value.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta") => {
            return Ok(OpenAiStreamEvent::Delta(
                value
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ));
        }
        Some("response.output_text.done") => {
            return Ok(OpenAiStreamEvent::FinalText(
                value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ));
        }
        Some("response.content_part.done") => {
            return Ok(OpenAiStreamEvent::FinalText(
                value
                    .pointer("/part/text")
                    .or_else(|| value.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ));
        }
        Some("response.output_item.done") => {
            return Ok(OpenAiStreamEvent::FinalText(response_output_item_text(
                value.get("item"),
            )));
        }
        Some("response.completed" | "response.done") => return Ok(OpenAiStreamEvent::Done),
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
    if let Some(usage) = token_usage_from_value(&value) {
        return Ok(OpenAiStreamEvent::Usage(usage));
    }
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("delta").and_then(Value::as_str))
        .or_else(|| value.get("output_text").and_then(Value::as_str))
        .unwrap_or_default();
    Ok(OpenAiStreamEvent::Delta(text.to_owned()))
}

fn response_output_item_text(item: Option<&Value>) -> String {
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
