use std::collections::BTreeMap;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::error::{Error, Result};

pub(super) async fn read(stream: &mut TcpStream) -> Result<(BTreeMap<String, String>, String)> {
    let mut bytes = Vec::new();
    let split = loop {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Err(Error::Protocol("webhook request ended early".to_owned()));
        }
        let part = chunk
            .get(..count)
            .ok_or_else(|| Error::Protocol("invalid read size".to_owned()))?;
        bytes.extend_from_slice(part);
        if let Some(index) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
            break index;
        }
        if bytes.len() > 1_048_576 {
            return Err(Error::Protocol("webhook headers are too large".to_owned()));
        }
    };
    let header_end = split + 4;
    let header_text = String::from_utf8_lossy(
        bytes
            .get(..split)
            .ok_or_else(|| Error::Protocol("invalid webhook headers".to_owned()))?,
    );
    let mut headers = BTreeMap::new();
    for line in header_text.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0_usize);
    if length > 1_048_576 {
        return Err(Error::Protocol("webhook body is too large".to_owned()));
    }
    while bytes.len() < header_end + length {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Err(Error::Protocol("webhook body ended early".to_owned()));
        }
        let part = chunk
            .get(..count)
            .ok_or_else(|| Error::Protocol("invalid read size".to_owned()))?;
        bytes.extend_from_slice(part);
    }
    let body = bytes
        .get(header_end..header_end + length)
        .ok_or_else(|| Error::Protocol("invalid webhook body".to_owned()))?;
    Ok((headers, String::from_utf8_lossy(body).into_owned()))
}

pub(super) async fn respond(stream: &mut TcpStream, status: &str) -> Result<()> {
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}
