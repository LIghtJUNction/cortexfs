use std::io::Write;
use std::net::{TcpListener, TcpStream};

pub mod parse;

pub use parse::{HttpRequest, read_request};

const MAX_HTTP_BODY_BYTES: usize = 1024 * 1024;

/// Minimal HTTP response used by the foreground webhook host.
#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl HttpResponse {
    #[must_use]
    pub fn json(body: String) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body,
        }
    }

    #[must_use]
    pub fn error(status: u16, body: &'static str) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: body.to_owned(),
        }
    }
}

/// Errors in the intentionally small webhook HTTP boundary.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("HTTP I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid HTTP request: {0}")]
    Invalid(String),
}

/// Serves exactly one request; callers own the explicit foreground accept loop.
pub fn serve_once(
    listener: &TcpListener,
    handler: impl FnOnce(HttpRequest) -> HttpResponse,
) -> Result<(), HttpError> {
    let (mut stream, _) = listener.accept()?;
    let request = match read_request(&mut stream, MAX_HTTP_BODY_BYTES) {
        Ok(request) => request,
        Err(error) => {
            write_response(&mut stream, &HttpResponse::error(400, "invalid request"))?;
            return Err(HttpError::Invalid(error.to_string()));
        }
    };
    let response = handler(request);
    write_response(&mut stream, &response)
}

fn write_response(stream: &mut TcpStream, response: &HttpResponse) -> Result<(), HttpError> {
    let status = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Response",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        status,
        response.content_type,
        response.body.len(),
        response.body
    )?;
    Ok(())
}
