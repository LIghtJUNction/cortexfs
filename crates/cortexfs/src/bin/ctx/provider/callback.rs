use crate::*;

pub(crate) const MAX_OAUTH_CALLBACK_REQUEST_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OAuthRedirect {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
}

pub(crate) fn parse_oauth_redirect_uri(value: &str) -> Result<OAuthRedirect, CliError> {
    let rest = value
        .strip_prefix("http://")
        .ok_or_else(|| CliError::usage("oauth redirect_uri must use http:// localhost"))?;
    let (authority, path) = rest.split_once('/').map_or_else(
        || (rest, "/".to_owned()),
        |(authority, path)| (authority, format!("/{path}")),
    );
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| CliError::usage("oauth redirect_uri must include a port"))?;
    if !matches!(host, "127.0.0.1" | "localhost") {
        return Err(CliError::usage(
            "oauth redirect_uri must bind localhost or 127.0.0.1",
        ));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_error| CliError::usage("oauth redirect_uri has invalid port"))?;
    Ok(OAuthRedirect {
        host: host.to_owned(),
        port,
        path,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OAuthCallbackParams {
    pub(crate) code: Option<String>,
    pub(crate) state: Option<String>,
}

pub(crate) fn read_oauth_callback_request(
    stream: &mut std::net::TcpStream,
    deadline: std::time::Instant,
) -> Result<String, CliError> {
    let read_timeout = deadline
        .checked_duration_since(std::time::Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| CliError::unavailable("oauth callback timed out"))?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|error| {
            CliError::unavailable(format!("cannot configure oauth callback: {error}"))
        })?;
    read_oauth_callback_request_from_reader(stream, MAX_OAUTH_CALLBACK_REQUEST_BYTES)
}

pub(crate) fn read_oauth_callback_request_from_reader(
    mut reader: impl Read,
    max_bytes: usize,
) -> Result<String, CliError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let size = reader
            .read(&mut chunk)
            .map_err(|error| match error.kind() {
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                    CliError::unavailable("oauth callback timed out")
                }
                _ => CliError::unavailable(format!("cannot read oauth callback: {error}")),
            })?;
        if size == 0 {
            break;
        }
        let Some(read_bytes) = chunk.get(..size) else {
            return Err(CliError::unavailable("oauth callback exceeded buffer"));
        };
        bytes.extend_from_slice(read_bytes);
        if bytes.len() > max_bytes {
            return Err(CliError::unavailable("oauth callback exceeded buffer"));
        }
        if let Some(end) = oauth_callback_headers_end(&bytes) {
            bytes.truncate(end);
            break;
        }
    }
    String::from_utf8(bytes).map_err(|_error| CliError::usage("oauth callback must be valid UTF-8"))
}

pub(crate) fn oauth_callback_headers_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .or_else(|| {
            bytes
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|position| position + 2)
        })
}

pub(crate) fn parse_oauth_callback_params(
    request: &str,
    expected_path: &str,
) -> Result<OAuthCallbackParams, CliError> {
    let Some(first_line) = request.lines().next() else {
        return Err(CliError::usage("empty oauth callback"));
    };
    let mut fields = first_line.split_whitespace();
    let method = fields.next().unwrap_or_default();
    let target = fields.next().unwrap_or_default();
    let version = fields.next().unwrap_or_default();
    if method != "GET" {
        return Err(CliError::usage("oauth callback must use GET"));
    }
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") || fields.next().is_some() {
        return Err(CliError::usage("oauth callback request line is invalid"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != expected_path {
        return Err(CliError::usage("oauth callback path mismatch"));
    }
    let mut code = None;
    let mut state = None;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value)?;
        match key {
            "code" if value.is_empty() => {
                return Err(CliError::usage("oauth callback empty code"));
            }
            "code" if code.is_none() => code = Some(value),
            "code" => return Err(CliError::usage("oauth callback repeated code")),
            "state" if value.is_empty() => {
                return Err(CliError::usage("oauth callback empty state"));
            }
            "state" if state.is_none() => state = Some(value),
            "state" => return Err(CliError::usage("oauth callback repeated state")),
            _ => {}
        }
    }
    Ok(OAuthCallbackParams { code, state })
}

pub(crate) fn percent_decode(value: &str) -> Result<String, CliError> {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let Some(&byte) = bytes.get(index) else {
            break;
        };
        match byte {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(CliError::usage("invalid oauth callback encoding"));
                }
                let Some(&high_raw) = bytes.get(index + 1) else {
                    return Err(CliError::usage("invalid oauth callback encoding"));
                };
                let Some(&low_raw) = bytes.get(index + 2) else {
                    return Err(CliError::usage("invalid oauth callback encoding"));
                };
                let high = hex_value(high_raw)?;
                let low = hex_value(low_raw)?;
                output.push((high << 4) | low);
                index += 3;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).map_err(|_error| CliError::usage("invalid oauth callback encoding"))
}

pub(crate) fn hex_value(byte: u8) -> Result<u8, CliError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(CliError::usage("invalid oauth callback encoding")),
    }
}

pub(crate) fn is_provider_name(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

pub(crate) fn curl_config_quote(value: &str) -> Result<String, CliError> {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if character.is_ascii_control() {
            return Err(CliError::usage(
                "curl config value contains a forbidden control character",
            ));
        }
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    Ok(quoted)
}

pub(crate) fn terminate_process_child(child: &mut std::process::Child) {
    match child.kill() {
        Ok(()) | Err(_) => {}
    }
    match child.wait() {
        Ok(_) | Err(_) => {}
    }
}
