use crate::object::executor::{cleanup_curl_child, spawn_child_stderr_reader};
use crate::object::runner::{
    MAX_PROVIDER_STREAM_LINE_BYTES, ProviderRequest, ResolvedTransport, call_openai_sse_streaming,
    provider_request_body, provider_request_target,
};
use cortexfs_protocol::WireProtocol;
use std::io::{self, BufRead, Read, Write};
use std::process::{Child, ChildStdout};
use std::thread;

pub(crate) type StreamFailure = crate::object::runner::ProviderCompletionError;

pub(crate) fn stream_failure(message: impl Into<String>, can_fallback: bool) -> StreamFailure {
    StreamFailure {
        message: message.into(),
        can_fallback,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OpenAiStreamApi {
    Chat,
    Responses,
}

impl OpenAiStreamApi {
    pub(crate) fn call_streaming(
        self,
        transport: &ResolvedTransport,
        request: &ProviderRequest<'_>,
        run: &str,
        stdout: &mut impl Write,
    ) -> Result<(), StreamFailure> {
        let protocol = self.protocol();
        let (target, headers) =
            provider_request_target(transport, request.credential, protocol, run)
                .map_err(|message| stream_failure(message, false))?;
        let body = provider_request_body(
            protocol,
            request.model,
            request.input,
            true,
            request.effort,
            std::env::var_os("CTX_AGENT").is_some(),
        )
        .map_err(|message| stream_failure(message, false))?;
        call_openai_sse_streaming(&target, &headers, &body, self, run, stdout)
    }

    fn protocol(self) -> WireProtocol {
        match self {
            Self::Chat => WireProtocol::OpenAiChat,
            Self::Responses => WireProtocol::OpenAiResponses,
        }
    }
}

pub(crate) fn provider_stream_pipes(
    child: &mut Child,
) -> Result<(ChildStdout, Option<thread::JoinHandle<String>>), StreamFailure> {
    let Some(stdout) = child.stdout.take() else {
        cleanup_curl_child(child);
        return Err(stream_failure("cannot read provider stream", true));
    };
    Ok((stdout, child.stderr.take().map(spawn_child_stderr_reader)))
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
