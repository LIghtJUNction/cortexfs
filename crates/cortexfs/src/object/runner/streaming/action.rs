use super::event::OpenAiStreamEvent;
use super::pipe::{StreamFailure, stream_failure};
use super::response;
use super::text::{OpenAiStreamTextEmitter, push_stream_text, write_stream_output};
use super::tool::{OpenAiToolCallStream, emit_openai_stream_tool_call};
use crate::object::executor::write_model_usage;
use std::io::Write;
use std::process::Child;

#[expect(
    clippy::struct_excessive_bools,
    reason = "stream protocol tracks independent API kind, terminal state, emission, and discarded response completion"
)]
pub(crate) struct StreamState<'a> {
    pub(crate) text: OpenAiStreamTextEmitter<'a>,
    pub(crate) tools: OpenAiToolCallStream,
    pub(crate) responses: bool,
    pub(crate) pending_response_call: Option<String>,
    pub(crate) response_done: bool,
    pub(crate) emitted: bool,
    pub(crate) done: bool,
}

pub(crate) fn apply_stream_event(
    state: &mut StreamState<'_>,
    event: OpenAiStreamEvent,
    terminal: bool,
    stdout: &mut impl Write,
    child: &mut Child,
) -> Result<(), StreamFailure> {
    match event {
        OpenAiStreamEvent::Delta(text) if !text.is_empty() => {
            push_stream_text(&mut state.text, stdout, &text, child)?;
            state.emitted = true;
        }
        OpenAiStreamEvent::FinalText(text)
            if !state.emitted && state.pending_response_call.is_none() && !text.is_empty() =>
        {
            push_stream_text(&mut state.text, stdout, &text, child)?;
            state.emitted = true;
        }
        OpenAiStreamEvent::Usage(usage) => write_stream_output(
            write_model_usage(stdout, state.text.run, usage).and_then(|()| stdout.flush()),
            child,
        )?,
        OpenAiStreamEvent::ToolCallDelta(delta) => {
            if state.responses {
                return Err(protocol_failure(
                    state,
                    child,
                    "Responses stream included a Chat tool call",
                ));
            }
            state.tools.push(delta);
        }
        OpenAiStreamEvent::ToolCall(call) => response::queue(state, child, call)?,
        OpenAiStreamEvent::ResponseCompleted(usage) => {
            response::complete(state, child, stdout, usage)?;
        }
        OpenAiStreamEvent::ResponseDone => response::discard(state, child)?,
        OpenAiStreamEvent::Delta(_)
        | OpenAiStreamEvent::FinalText(_)
        | OpenAiStreamEvent::Done
        | OpenAiStreamEvent::Ignore => {}
    }
    if terminal
        && write_stream_output(
            emit_openai_stream_tool_call(stdout, &mut state.text, &mut state.tools),
            child,
        )?
    {
        state.emitted = true;
    }
    if terminal {
        state.done = true;
    }
    Ok(())
}

pub(super) fn protocol_failure(
    state: &StreamState<'_>,
    child: &mut Child,
    message: &str,
) -> StreamFailure {
    crate::object::executor::cleanup_curl_child(child);
    stream_failure(message.to_owned(), !state.emitted)
}
