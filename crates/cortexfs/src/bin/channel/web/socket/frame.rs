use std::net::TcpStream;

use cortexfs_runtime_client::interaction::{
    InteractionEvent, InteractionFrame, InteractionPayload, InteractionRequest,
};
use tungstenite::{Message, WebSocket};

use super::super::WebError;

pub(super) fn send_event(
    socket: &mut WebSocket<TcpStream>,
    event: InteractionEvent,
) -> Result<(), WebError> {
    let frame = InteractionFrame::event(event)
        .encode()
        .map_err(|_error| WebError::InvalidFrame)?;
    let text = String::from_utf8(frame)
        .map_err(|_error| WebError::InvalidFrame)?
        .trim_end_matches('\n')
        .to_owned();
    socket.send(Message::text(text))?;
    Ok(())
}

pub(super) fn read_request(
    socket: &mut WebSocket<TcpStream>,
) -> Result<InteractionRequest, WebError> {
    loop {
        let message = socket.read()?;
        match message {
            Message::Text(text) => return decode_request(text.as_bytes()),
            Message::Binary(bytes) => return decode_request(bytes.as_ref()),
            Message::Ping(payload) => socket.send(Message::Pong(payload))?,
            Message::Close(_) => return Err(WebError::Closed),
            Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}

pub(super) fn decode_message(message: Message) -> Result<Option<InteractionRequest>, WebError> {
    match message {
        Message::Text(text) => decode_request(text.as_bytes()).map(Some),
        Message::Binary(bytes) => decode_request(bytes.as_ref()).map(Some),
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Ok(None),
        Message::Close(_) => Err(WebError::Closed),
    }
}

fn decode_request(bytes: &[u8]) -> Result<InteractionRequest, WebError> {
    let mut line = Vec::with_capacity(bytes.len().saturating_add(1));
    line.extend_from_slice(bytes);
    line.push(b'\n');
    let frame = InteractionFrame::decode(&line).map_err(|_error| WebError::InvalidFrame)?;
    let InteractionPayload::Request(request) = frame.payload else {
        return Err(WebError::InvalidFrame);
    };
    Ok(request)
}
