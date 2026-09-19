use std::collections::BTreeMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::{Error, Result};

fn content_length(value: Option<&str>) -> Result<usize> {
    let value = value.unwrap_or("0");
    if value.starts_with('+') {
        return Err(Error::Protocol("invalid content length".into()));
    }
    value.parse().map_err(|error| Error::Protocol(error.to_string()))
}

pub(super) async fn read(stream: &mut TcpStream) -> Result<(BTreeMap<String, String>, String)> {
    let mut bytes = Vec::with_capacity(4096);
    let split = loop {
        let count = stream.read_buf(&mut bytes).await?;
        if count == 0 {
            return Err(Error::Protocol("webhook request ended early".to_owned()));
        }
        if let Some(index) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
            break index;
        }
        if bytes.len() > 1_048_576 {
            return Err(Error::Protocol("webhook headers are too large".to_owned()));
        }
    };
    let header_end = split + 4;
    let header = bytes
        .get(..split)
        .ok_or(Error::Protocol("invalid webhook headers".into()))?;
    let header_text = String::from_utf8_lossy(header);
    let mut headers = BTreeMap::new();
    for line in header_text.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length = content_length(headers.get("content-length").map(String::as_str))?;
    if length > 1_048_576 {
        return Err(Error::Protocol("webhook body is too large".to_owned()));
    }
    while bytes.len() < header_end + length {
        let count = stream.read_buf(&mut bytes).await?;
        if count == 0 {
            return Err(Error::Protocol("webhook body ended early".to_owned()));
        }
    }
    let body = bytes
        .get(header_end..header_end + length)
        .ok_or(Error::Protocol("invalid webhook body".into()))?;
    Ok((headers, String::from_utf8_lossy(body).into_owned()))
}

pub(super) async fn respond(stream: &mut TcpStream, status: &str) -> Result<()> {
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
#[test]
fn content_length_fails_closed() {
    assert_eq!(content_length(None).unwrap(), 0);
    assert_eq!(content_length(Some("2")).unwrap(), 2);
    let invalid = ["x", "+2", "184467440737095516160"];
    assert!(invalid.into_iter().all(|value| content_length(Some(value)).is_err()));
}
