use std::io::{Error, ErrorKind, Read};
use std::net::TcpStream;

fn invalid(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: std::collections::BTreeMap<String, String>,
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
        bytes.extend(buffer.iter().take(read).copied());
        if bytes.len() > 64 * 1024 {
            return Err(invalid("HTTP headers too large"));
        }
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let header = std::str::from_utf8(bytes.get(..header_end).unwrap_or_default())
        .map_err(|_error| invalid("HTTP headers are not UTF-8"))?;
    let mut lines = header.split("\r\n");
    let mut request = lines.next().unwrap_or_default().split(' ');
    let method = request.next().unwrap_or_default().to_owned();
    let path = request.next().unwrap_or_default().to_owned();
    let version = request.next().unwrap_or_default();
    if path.is_empty()
        || path.bytes().any(|byte| byte.is_ascii_whitespace())
        || reqwest::header::HeaderName::from_bytes(method.as_bytes()).is_err()
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || request.next().is_some()
    {
        return Err(invalid("invalid HTTP request line"));
    }
    let mut headers = std::collections::BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .filter(|&(name, value)| {
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_ok()
                    && value
                        .bytes()
                        .all(|byte| !byte.is_ascii_control() || byte == b'\t')
            })
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .ok_or_else(|| invalid("invalid HTTP header"))?;
        if name == "transfer-encoding" {
            return Err(invalid("invalid HTTP framing"));
        }
        if headers.insert(name, value.trim().to_owned()).is_some() {
            return Err(invalid("duplicate HTTP header"));
        }
    }
    if version == "HTTP/1.1" && !headers.contains_key("host") {
        return Err(invalid("invalid HTTP host"));
    }
    let length = match headers.get("content-length") {
        None => 0,
        Some(value) if value.bytes().all(|byte| byte.is_ascii_digit()) => value
            .parse::<usize>()
            .map_err(|_error| invalid("invalid content length"))?,
        Some(_) => return Err(invalid("invalid content length")),
    };
    if length > max_body {
        return Err(invalid("HTTP body too large"));
    }
    let mut body = bytes.get(header_end..).unwrap_or_default().to_vec();
    let have = body.len().min(length);
    body.resize(length, 0);
    stream.read_exact(body.get_mut(have..).unwrap_or_default())?;
    Ok(HttpRequest {
        method,
        path,
        headers,
        body: String::from_utf8(body).map_err(|_error| invalid("HTTP body is not UTF-8"))?,
    })
}
