use super::action::{StreamState, apply_stream_event};
use super::close::close_stream;
use super::event::openai_stream_event;
use super::pipe::{
    OpenAiStreamApi, StreamFailure, provider_stream_pipes, read_provider_stream_line,
    stream_failure,
};
use super::text::OpenAiStreamTextEmitter;
use super::tool::OpenAiToolCallStream;
use crate::object::executor::cleanup_curl_child;
use crate::object::runner::{CurlJsonTarget, start_curl_json_with_headers};
use std::io::{BufReader, Write};

pub(crate) fn call_openai_sse_streaming(
    target: &CurlJsonTarget,
    headers: &[String],
    body: &str,
    api: OpenAiStreamApi,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let mut child = start_curl_json_with_headers(target, headers, body)
        .map_err(|message| stream_failure(message, true))?;
    let (child_stdout, stderr_reader) = provider_stream_pipes(&mut child)?;
    let mut state = StreamState {
        text: OpenAiStreamTextEmitter::new(run),
        tools: OpenAiToolCallStream::default(),
        responses: api == OpenAiStreamApi::Responses,
        pending_response_call: None,
        response_done: false,
        emitted: false,
        done: false,
    };
    let mut stream = BufReader::new(child_stdout);
    loop {
        let line = read_provider_stream_line(&mut stream).map_err(|error| {
            cleanup_curl_child(&mut child);
            stream_failure(
                format!("cannot read provider stream: {error}"),
                !state.emitted,
            )
        })?;
        let Some(line) = line else { break };
        let frame = openai_stream_event(&line).map_err(|message| {
            cleanup_curl_child(&mut child);
            stream_failure(message, !state.emitted)
        })?;
        let terminal = frame.terminal && (!frame.chat_terminal || api == OpenAiStreamApi::Chat);
        apply_stream_event(&mut state, frame.event, terminal, stdout, &mut child)?;
    }
    close_stream(&mut child, stderr_reader, &mut state, stdout)
}
