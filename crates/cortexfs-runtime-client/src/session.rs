use std::path::Path;

use serde_json::json;

use crate::{RuntimeClientError, interaction};

mod duplex;

pub(super) const MAX_SESSION_FRAME_BYTES: usize = interaction::MAX_INTERACTION_FRAME_BYTES;
pub(super) const MAX_SESSION_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

/// Stable `send` request fields for an agent session socket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionSendRequest<'a> {
    pub request_id: &'a str,
    pub session: &'a str,
    pub scope: &'a str,
    pub cwd: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub input: &'a str,
}

/// Sends one session request and returns canonical JSONL events.
pub fn send(
    socket: &Path,
    request: SessionSendRequest<'_>,
) -> Result<Vec<String>, RuntimeClientError> {
    let mut frames = Vec::new();
    send_stream(socket, request, |frame| {
        frames.push(frame.to_owned());
        Ok::<(), RuntimeClientError>(())
    })?;
    Ok(frames)
}

/// Sends one session request and calls `on_frame` for each event.
pub fn send_stream<F, E>(
    socket: &Path,
    request: SessionSendRequest<'_>,
    mut on_frame: F,
) -> Result<(), E>
where
    F: FnMut(&str) -> Result<(), E>,
    E: From<RuntimeClientError>,
{
    validate(&request).map_err(E::from)?;
    let frame = json!({
        "op": "send",
        "id": request.request_id,
        "session": request.session,
        "scope": request.scope,
        "cwd": request.cwd,
        "workspace": request.workspace,
        "input": request.input,
    })
    .to_string();
    duplex::send_json_stream_with(socket, &frame, |_stream, line| on_frame(line))
}

/// Sends one provider-neutral interaction request through the Agent socket.
pub fn send_interaction_stream<F, E>(
    socket: &Path,
    request: interaction::InteractionRequest,
    mut on_frame: F,
) -> Result<(), E>
where
    F: FnMut(&str) -> Result<(), E>,
    E: From<RuntimeClientError>,
{
    let frame = interaction::InteractionFrame::request(request);
    frame
        .validate()
        .map_err(|_error| E::from(RuntimeClientError::InvalidRequest))?;
    let frame = serde_json::to_string(&frame)
        .map_err(|_error| E::from(RuntimeClientError::InvalidRequest))?;
    duplex::send_json_stream_with(socket, &frame, |_stream, line| on_frame(line))
}

/// Sends a request and normalizes executable-Agent events.
/// Input streams must finish with `done` or a nonrecoverable error.
pub fn send_interaction_events<F, E>(
    socket: &Path,
    request: interaction::InteractionRequest,
    mut on_event: F,
) -> Result<(), E>
where
    F: FnMut(interaction::InteractionEvent) -> Result<(), E>,
    E: From<RuntimeClientError>,
{
    send_events_with(socket, request, |_stream, event| on_event(event))
}

/// Sends a request and answers runtime commands on the same socket.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "match ergonomics keep borrowed interaction events readable"
)]
pub fn send_interaction_events_with_commands<F, C, E>(
    socket: &Path,
    request: interaction::InteractionRequest,
    mut on_event: F,
    mut on_command: C,
) -> Result<(), E>
where
    F: FnMut(interaction::InteractionEvent) -> Result<(), E>,
    C: FnMut(&interaction::InteractionEvent) -> Result<interaction::InteractionResult, E>,
    E: From<RuntimeClientError>,
{
    let request_id = request.request_id().to_owned();
    let session = request.session().unwrap_or("default").to_owned();
    send_events_with(socket, request, |stream, event| {
        on_event(event.clone())?;
        if let interaction::InteractionEvent::Command { command_id, .. } = &event {
            let result = on_command(&event)?;
            duplex::write_interaction_request(
                stream,
                interaction::InteractionRequest::CommandResult {
                    request_id: request_id.clone(),
                    session: session.clone(),
                    command_id: command_id.clone(),
                    result,
                },
            )
            .map_err(E::from)?;
        }
        Ok(())
    })
}

fn send_events_with<F, E>(
    socket: &Path,
    request: interaction::InteractionRequest,
    mut on_event: F,
) -> Result<(), E>
where
    F: FnMut(&mut std::os::unix::net::UnixStream, interaction::InteractionEvent) -> Result<(), E>,
    E: From<RuntimeClientError>,
{
    let request_id = request.request_id().to_owned();
    let input = matches!(request, interaction::InteractionRequest::Input { .. });
    let mut complete = !input;
    let frame = interaction::InteractionFrame::request(request)
        .encode()
        .map_err(|_error| E::from(RuntimeClientError::InvalidRequest))?;
    let frame = std::str::from_utf8(&frame)
        .map_err(|_error| E::from(RuntimeClientError::InvalidRequest))?;
    duplex::send_json_stream_with(socket, frame.trim_end_matches('\n'), |stream, raw| {
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|_error| E::from(RuntimeClientError::InvalidFrame))?;
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| E::from(RuntimeClientError::InvalidFrame))?;
        if input
            && value
                .get("request_id")
                .is_some_and(|id| id.as_str() != Some(request_id.as_str()))
        {
            return Err(E::from(RuntimeClientError::InvalidFrame));
        }
        if let Some(event) = interaction::interaction_event_from_agent_frame(&request_id, raw) {
            complete |= matches!(
                event,
                interaction::InteractionEvent::Done { .. }
                    | interaction::InteractionEvent::Error {
                        retryable: false,
                        ..
                    }
            );
            on_event(stream, event)?;
        } else if matches!(kind, "delta" | "tool_call" | "approval_request") {
            return Err(E::from(RuntimeClientError::InvalidFrame));
        }
        Ok(())
    })?;
    complete
        .then_some(())
        .ok_or_else(|| E::from(RuntimeClientError::InvalidFrame))
}

fn validate(request: &SessionSendRequest<'_>) -> Result<(), RuntimeClientError> {
    if !matches!(request.scope, "private" | "shared" | "temp") {
        return Err(RuntimeClientError::InvalidRequest);
    }
    if [request.request_id, request.session, request.input]
        .iter()
        .any(|field| field.is_empty() || field.contains('\0'))
        || request.cwd.is_some_and(|value| value.contains('\0'))
        || request.workspace.is_some_and(|value| value.contains('\0'))
    {
        return Err(RuntimeClientError::InvalidRequest);
    }
    Ok(())
}
