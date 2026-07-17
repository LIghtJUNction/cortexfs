use crate::*;

pub(crate) const MAX_OAUTH_CALLBACK_REQUEST_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OAuthRedirect {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
}

pub(crate) fn parse_oauth_redirect_uri(value: &str) -> Result<OAuthRedirect, CliError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_error| CliError::usage("oauth redirect_uri must use http:// localhost"))?;
    if url.scheme() != "http" || url.username() != "" || url.password().is_some() {
        return Err(CliError::usage(
            "oauth redirect_uri must use http:// localhost",
        ));
    }
    let host = url.host_str().unwrap_or_default();
    if !matches!(host, "127.0.0.1" | "localhost") {
        return Err(CliError::usage(
            "oauth redirect_uri must bind localhost or 127.0.0.1",
        ));
    }
    let port = url
        .port()
        .ok_or_else(|| CliError::usage("oauth redirect_uri must include a port"))?;
    Ok(OAuthRedirect {
        host: host.to_owned(),
        port,
        path: url.path().to_owned(),
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
    reader: impl Read,
    max_bytes: usize,
) -> Result<String, CliError> {
    let mut reader = io::BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        let size =
            BufRead::read_until(&mut reader, b'\n', &mut bytes).map_err(|error| {
                match error.kind() {
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                        CliError::unavailable("oauth callback timed out")
                    }
                    _ => CliError::unavailable(format!("cannot read oauth callback: {error}")),
                }
            })?;
        if bytes.len() > max_bytes {
            return Err(CliError::unavailable("oauth callback exceeded buffer"));
        }
        if size == 0 || bytes.ends_with(b"\r\n\r\n") || bytes.ends_with(b"\n\n") {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|_error| CliError::usage("oauth callback must be valid UTF-8"))
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
    let url = reqwest::Url::parse(&format!("http://localhost{target}"))
        .map_err(|_error| CliError::usage("oauth callback request target is invalid"))?;
    if url.path() != expected_path {
        return Err(CliError::usage("oauth callback path mismatch"));
    }
    let mut code = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" if value.is_empty() => {
                return Err(CliError::usage("oauth callback empty code"));
            }
            "code" if code.is_none() => code = Some(value.into_owned()),
            "code" => return Err(CliError::usage("oauth callback repeated code")),
            "state" if value.is_empty() => {
                return Err(CliError::usage("oauth callback empty state"));
            }
            "state" if state.is_none() => state = Some(value.into_owned()),
            "state" => return Err(CliError::usage("oauth callback repeated state")),
            _ => {}
        }
    }
    Ok(OAuthCallbackParams { code, state })
}

pub(crate) fn is_provider_name(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}
