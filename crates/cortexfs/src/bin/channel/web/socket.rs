use std::net::TcpStream;

use tungstenite::{
    accept_hdr,
    http::{HeaderMap, Response},
};

use super::{WebConfig, WebError};

mod frame;
mod pump;
type Rejection = Response<Option<String>>;

pub(super) fn is_upgrade(stream: &TcpStream) -> bool {
    let mut bytes = [0_u8; 8 * 1024];
    let Ok(size) = stream.peek(&mut bytes) else {
        return false;
    };
    String::from_utf8_lossy(bytes.get(..size).unwrap_or(&bytes))
        .to_ascii_lowercase()
        .contains("upgrade: websocket")
}

#[expect(
    clippy::result_large_err,
    reason = "tungstenite's callback ABI requires its complete HTTP rejection"
)]
pub(super) fn serve(stream: TcpStream, config: &WebConfig) -> Result<(), WebError> {
    let path = config.path.clone();
    let token = config.token.clone();
    let mut socket = accept_hdr(
        stream,
        move |request: &tungstenite::handshake::server::Request,
              response: tungstenite::handshake::server::Response| {
            if request.uri().path() != path {
                return Err(rejection(404, "not found"));
            }
            if !authorized(request.headers(), token.as_deref()) {
                return Err(rejection(401, "unauthorized"));
            }
            Ok(response)
        },
    )
    .map_err(|error| WebError::Handshake(error.to_string()))?;
    let request = frame::read_request(&mut socket)?;
    pump::serve(socket, config, &request)
}

fn authorized(headers: &HeaderMap, token: Option<&str>) -> bool {
    let Some(token) = token else {
        return true;
    };
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        == Some(token)
}

fn rejection(status: u16, body: &'static str) -> Rejection {
    Response::builder()
        .status(status)
        .body(Some(body.to_owned()))
        .unwrap_or_else(|_error| Response::new(None))
}
