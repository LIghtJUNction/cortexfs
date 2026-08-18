use std::{fmt, net::SocketAddr, path::PathBuf};

use cortexfs::channel::http::{HttpRequest, HttpResponse};
use cortexfs_runtime_client::{
    RuntimeClientError,
    interaction::{InteractionFrame, InteractionPayload, InteractionResult},
    session,
};

mod server;
mod socket;

#[cfg(test)]
mod tests;

/// Foreground HTTP host for the provider-neutral interaction ABI.
#[derive(Clone)]
pub struct WebConfig {
    pub socket: PathBuf,
    pub bind: SocketAddr,
    pub path: String,
    pub token: Option<String>,
}

impl fmt::Debug for WebConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebConfig")
            .field("socket", &self.socket)
            .field("bind", &self.bind)
            .field("path", &self.path)
            .field("token", &self.token.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

pub fn run(config: &WebConfig) -> Result<(), WebError> {
    server::run(config)
}

#[derive(Debug, thiserror::Error)]
pub enum WebError {
    #[error("web I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("websocket failed: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("websocket handshake failed: {0}")]
    Handshake(String),
    #[error("agent interaction failed: {0}")]
    Runtime(#[from] RuntimeClientError),
    #[error("invalid web interaction frame")]
    InvalidFrame,
    #[error("websocket closed")]
    Closed,
}

fn handle(config: &WebConfig, request: &HttpRequest) -> HttpResponse {
    if request.method != "POST" || request.path != config.path {
        return HttpResponse::error(404, "not found");
    }
    if !authorized(request, config.token.as_deref()) {
        return HttpResponse::error(401, "unauthorized");
    }
    let Ok(frame) = serde_json::from_str::<InteractionFrame>(&request.body) else {
        return HttpResponse::error(400, "invalid interaction frame");
    };
    if frame.validate().is_err() {
        return HttpResponse::error(400, "invalid interaction frame");
    }
    let InteractionPayload::Request(request) = frame.payload else {
        return HttpResponse::error(400, "interaction event is not a request");
    };
    let mut body = String::new();
    let result = session::send_interaction_events_with_commands(
        &config.socket,
        request,
        |event| {
            let frame = InteractionFrame::event(event);
            let line =
                serde_json::to_string(&frame).map_err(|_error| RuntimeClientError::InvalidFrame)?;
            body.push_str(&line);
            body.push('\n');
            Ok::<(), RuntimeClientError>(())
        },
        |_event| {
            Ok(InteractionResult::Rejected {
                reason: "web POST requires a bidirectional streaming transport".to_owned(),
            })
        },
    );
    match result {
        Ok(()) => HttpResponse::ndjson(body),
        Err(_error) => HttpResponse::error(502, "agent interaction failed"),
    }
}

fn authorized(request: &HttpRequest, token: Option<&str>) -> bool {
    let Some(token) = token else {
        return true;
    };
    request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        == Some(token)
}
