use super::action::StreamState;
use super::pipe::{StreamFailure, stream_failure};
use super::tool::emit_openai_stream_tool_call;
use crate::object::executor::collect_child_stderr;
use std::io::Write;
use std::process::ExitStatus;
use std::thread;

pub(crate) fn close_stream(
    child: &mut std::process::Child,
    stderr_reader: Option<thread::JoinHandle<String>>,
    state: &mut StreamState<'_>,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let status = child
        .wait()
        .map_err(|error| stream_failure(format!("cannot run curl: {error}"), !state.emitted))?;
    if !status.success() {
        let stderr = collect_child_stderr(stderr_reader);
        let message = provider_stream_failure_message(status, &stderr);
        if state.emitted {
            finish_stream_output(state, stdout)?;
            return Err(stream_failure(message, false));
        }
        return Err(stream_failure(message, true));
    }
    if !state.responses
        && emit_openai_stream_tool_call(stdout, &mut state.text, &mut state.tools)
            .map_err(|error| stream_output_failure(&error))?
    {
        state.emitted = true;
    }
    finish_stream_output(state, stdout)?;
    if state.emitted && state.done {
        Ok(())
    } else {
        let message = if state.done {
            "provider stream produced no answer text"
        } else if state.emitted {
            "provider stream ended without a terminal event"
        } else {
            "provider stream produced no content"
        };
        Err(stream_failure(message, !state.emitted))
    }
}

fn stream_output_failure(error: &std::io::Error) -> StreamFailure {
    stream_failure(format!("cannot write output: {error}"), false)
}

fn finish_stream_output(
    state: &mut StreamState<'_>,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    state
        .text
        .finish(stdout)
        .and_then(|()| stdout.flush())
        .map_err(|error| stream_output_failure(&error))
}

pub(crate) fn provider_stream_failure_message(status: ExitStatus, stderr: &str) -> String {
    if stderr.trim().is_empty() {
        format!("provider stream request failed with {status}")
    } else {
        format!(
            "provider stream request failed with {status}: {}",
            stderr.trim()
        )
    }
}
