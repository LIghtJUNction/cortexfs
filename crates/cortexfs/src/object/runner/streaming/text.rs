use super::pipe::{StreamFailure, stream_failure};
use crate::object::executor::{
    cleanup_curl_child, write_model_delta, write_model_text_or_tool_call,
};
use crate::object::runner::MAX_STREAM_TOOL_CALL_BUFFER_BYTES;
use std::io::{self, Write};
use std::process::Child;

pub(crate) enum StreamTextMode {
    Undecided,
    BufferToolCall,
    Plain,
}

pub(crate) struct OpenAiStreamTextEmitter<'a> {
    pub(crate) run: &'a str,
    mode: StreamTextMode,
    buffer: String,
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
                reject_oversized_stream_tool_call_buffer(&self.buffer)
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
                write_model_delta(stdout, self.run, &std::mem::take(&mut self.buffer))
            }
        }
    }

    pub(crate) fn finish(&mut self, stdout: &mut impl Write) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        write_model_text_or_tool_call(stdout, self.run, &std::mem::take(&mut self.buffer))
    }
}

pub(crate) fn push_stream_text(
    emitter: &mut OpenAiStreamTextEmitter<'_>,
    stdout: &mut impl Write,
    text: &str,
    child: &mut Child,
) -> Result<(), StreamFailure> {
    write_stream_output(
        emitter.push(stdout, text).and_then(|()| stdout.flush()),
        child,
    )
}

pub(crate) fn write_stream_output<T>(
    result: io::Result<T>,
    child: &mut Child,
) -> Result<T, StreamFailure> {
    result.map_err(|error| {
        cleanup_curl_child(child);
        stream_failure(format!("cannot write output: {error}"), false)
    })
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
