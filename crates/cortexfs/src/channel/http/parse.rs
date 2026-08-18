use std::collections::BTreeMap;
use std::io::{Error, ErrorKind, Read};
use std::net::TcpStream;

const MAX_HEADER_BYTES: usize = 64 * 1024;

/// Parsed request data needed by a platform webhook codec.
#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

pub fn read_request(stream: &mut TcpStream, max_body: usize) -> Result<HttpRequest, Error> {
    let mut bytes = Vec::with_capacity(4096);
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, "missing HTTP headers"));
        }
        bytes.extend_from_slice(
            buffer
                .get(..read)
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "HTTP read exceeded buffer"))?,
        );
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(Error::new(ErrorKind::InvalidData, "HTTP headers too large"));
        }
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let header = std::str::from_utf8(
        bytes
            .get(..header_end)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "invalid HTTP header range"))?,
    )
    .map_err(|_error| Error::new(ErrorKind::InvalidData, "HTTP headers are not UTF-8"))?;
    let mut lines = header.split("\r\n");
    let mut request = lines
        .next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing HTTP request line"))?
        .split_whitespace();
    let method = request.next().unwrap_or_default().to_owned();
    let path = request.next().unwrap_or_default().to_owned();
    if method.is_empty() || path.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "invalid HTTP request line",
        ));
    }
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "invalid HTTP header"))?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
    }
    let length = headers.get("content-length").map_or(Ok(0), |value| {
        value
            .parse::<usize>()
            .map_err(|_error| Error::new(ErrorKind::InvalidData, "invalid content length"))
    })?;
    if length > max_body {
        return Err(Error::new(ErrorKind::InvalidData, "HTTP body too large"));
    }
    let mut body = bytes.get(header_end..).unwrap_or_default().to_vec();
    while body.len() < length {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, "truncated HTTP body"));
        }
        body.extend_from_slice(
            buffer.get(..read).ok_or_else(|| {
                Error::new(ErrorKind::InvalidData, "HTTP body read exceeded buffer")
            })?,
        );
    }
    body.truncate(length);
    let body = String::from_utf8(body)
        .map_err(|_error| Error::new(ErrorKind::InvalidData, "HTTP body is not UTF-8"))?;
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}
