use super::action::{StreamState, protocol_failure};
use super::pipe::StreamFailure;
use super::text::write_stream_output;
use crate::object::executor::{write_model_text_or_tool_call, write_model_usage};
use crate::object::runner::TokenUsage;
use std::io::Write;
use std::process::Child;

pub(crate) fn queue(
    state: &mut StreamState<'_>,
    child: &mut Child,
    call: String,
) -> Result<(), StreamFailure> {
    if !state.responses
        || state.done
        || state.response_done
        || state.pending_response_call.is_some()
    {
        return Err(protocol_failure(
            state,
            child,
            "Responses stream produced multiple or terminal tool calls",
        ));
    }
    state.pending_response_call = Some(call);
    Ok(())
}

pub(crate) fn complete(
    state: &mut StreamState<'_>,
    child: &mut Child,
    stdout: &mut impl Write,
    usage: Option<TokenUsage>,
) -> Result<(), StreamFailure> {
    if !state.responses || state.done || state.response_done {
        return Err(protocol_failure(
            state,
            child,
            "invalid Responses stream completion",
        ));
    }
    if let Some(call) = state.pending_response_call.take() {
        write_stream_output(
            state
                .text
                .finish(stdout)
                .and_then(|()| write_model_text_or_tool_call(stdout, state.text.run, &call))
                .and_then(|()| stdout.flush()),
            child,
        )?;
        state.emitted = true;
    }
    if let Some(usage) = usage {
        write_stream_output(
            write_model_usage(stdout, state.text.run, usage).and_then(|()| stdout.flush()),
            child,
        )?;
    }
    state.done = true;
    Ok(())
}

pub(crate) fn discard(state: &mut StreamState<'_>, child: &mut Child) -> Result<(), StreamFailure> {
    if !state.responses || state.done || state.response_done {
        return Err(protocol_failure(
            state,
            child,
            "invalid Responses stream completion",
        ));
    }
    state.pending_response_call = None;
    state.response_done = true;
    Ok(())
}
