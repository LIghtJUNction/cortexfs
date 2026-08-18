use std::io::{self, BufRead, Write};

use super::header::{parse_headers, read_line};
use super::policy::reject_programmatic_tools;
use super::{ProviderTarget, Request, invalid};

pub(super) fn parse_request(
    input: &mut impl BufRead,
    output: &mut impl Write,
    target: &ProviderTarget,
) -> io::Result<Request> {
    let request_line = read_line(input)?;
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if parts.next().is_some() || method != "POST" || version != "HTTP/1.1" {
        return Err(invalid("unsupported provider HTTP request line"));
    }
    let endpoint = endpoint_for_path(path, &target.base_path)?;
    let (headers, length, expect) = parse_headers(input, target, request_line.len() + 2)?;
    if expect {
        output.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        output.flush()?;
    }
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    reject_programmatic_tools(endpoint, &body)?;
    Ok(Request {
        endpoint,
        headers,
        body,
    })
}

fn endpoint_for_path(path: &str, base: &str) -> io::Result<&'static str> {
    if !path.starts_with('/')
        || path.contains(['?', '#', '\\'])
        || path.split('/').any(|part| part == "..")
    {
        return Err(invalid("invalid provider HTTP path"));
    }
    for endpoint in ["chat/completions", "responses", "messages"] {
        let expected = if base.is_empty() || base == "/" {
            format!("/{endpoint}")
        } else {
            format!("{base}/{endpoint}")
        };
        if path == expected {
            return Ok(endpoint);
        }
    }
    Err(invalid("unsupported provider HTTP path"))
}
