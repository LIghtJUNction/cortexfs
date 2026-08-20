#[cfg(test)]
use crate::*;

#[cfg(test)]
pub(crate) const MAX_OAUTH_CALLBACK_REQUEST_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(test)]
pub(crate) struct OAuthCallbackParams {
    pub(crate) code: Option<String>,
    pub(crate) state: Option<String>,
}

#[cfg(test)]
pub(crate) fn read_oauth_callback_request_from_reader(
    reader: impl Read,
    max: usize,
) -> Result<String, CliError> {
    let mut reader = io::BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        let size = BufRead::read_until(&mut reader, b'\n', &mut bytes).map_err(|error| {
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) {
                CliError::unavailable("oauth callback timed out")
            } else {
                CliError::unavailable("cannot read oauth callback")
            }
        })?;
        if bytes.len() > max {
            return Err(CliError::unavailable("oauth callback exceeded buffer"));
        }
        if size == 0 || bytes.ends_with(b"\r\n\r\n") || bytes.ends_with(b"\n\n") {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|_error| CliError::usage("oauth callback must be valid UTF-8"))
}

#[cfg(test)]
pub(crate) fn parse_oauth_callback_params(
    request: &str,
    expected: &str,
) -> Result<OAuthCallbackParams, CliError> {
    let line = request
        .lines()
        .next()
        .ok_or_else(|| CliError::usage("empty oauth callback"))?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if method != "GET" || !matches!(version, "HTTP/1.0" | "HTTP/1.1") || parts.next().is_some() {
        return Err(CliError::usage("oauth callback request line is invalid"));
    }
    let url = reqwest::Url::parse(&format!("http://localhost{target}"))
        .map_err(|_error| CliError::usage("oauth callback request target is invalid"))?;
    if url.path() != expected {
        return Err(CliError::usage("oauth callback path mismatch"));
    }
    let mut code = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" if value.is_empty() || code.is_some() => {
                return Err(CliError::usage("oauth callback invalid code"));
            }
            "code" => code = Some(value.into_owned()),
            "state" if value.is_empty() || state.is_some() => {
                return Err(CliError::usage("oauth callback invalid state"));
            }
            "state" => state = Some(value.into_owned()),
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
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
