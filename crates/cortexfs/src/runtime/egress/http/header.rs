use std::io::{self, BufRead};

use super::{ProviderTarget, invalid};

pub(super) const HEADER_LINE_MAX: usize = 8 * 1024;
const HEADER_TOTAL_MAX: usize = 32 * 1024;
pub(super) const HEADER_COUNT_MAX: usize = 64;
pub(super) const BODY_MAX: usize = 4 * 1024 * 1024;
type ParsedHeaders = (Vec<(String, String)>, usize, bool);

pub(super) fn parse_headers(
    input: &mut impl BufRead,
    target: &ProviderTarget,
    mut total: usize,
) -> io::Result<ParsedHeaders> {
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut length = None;
    let mut expect = false;
    let mut count = 0;
    loop {
        let line = read_line(input)?;
        total = total.saturating_add(line.len() + 2);
        if total > HEADER_TOTAL_MAX {
            return Err(invalid("provider HTTP headers exceed limit"));
        }
        if line.is_empty() {
            break;
        }
        count += 1;
        if count > HEADER_COUNT_MAX || line.starts_with([' ', '\t']) {
            return Err(invalid("invalid provider HTTP header"));
        }
        let (raw_name, raw_value) = line
            .split_once(':')
            .ok_or_else(|| invalid("invalid provider HTTP header"))?;
        if raw_name.is_empty()
            || !raw_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
            || raw_value
                .bytes()
                .any(|byte| (byte < b' ' && byte != b'\t') || byte == 0x7f)
        {
            return Err(invalid("invalid provider HTTP header"));
        }
        let name = raw_name.to_ascii_lowercase();
        let value = raw_value.trim_matches([' ', '\t']);
        match name.as_str() {
            "content-length" => {
                if length.is_some() || value.is_empty() {
                    return Err(invalid("duplicate provider HTTP content length"));
                }
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_error| invalid("invalid provider HTTP content length"))?;
                if parsed > BODY_MAX {
                    return Err(invalid("provider HTTP body exceeds limit"));
                }
                length = Some(parsed);
            }
            "expect" if value.eq_ignore_ascii_case("100-continue") => {
                if expect {
                    return Err(invalid("duplicate provider HTTP expectation"));
                }
                expect = true;
            }
            "authorization" | "x-api-key" | "anthropic-version" | "content-type" | "accept"
            | "chatgpt-account-id" | "originator" | "session-id" | "user-agent"
                if !matches!(
                    name.as_str(),
                    "chatgpt-account-id" | "originator" | "session-id" | "user-agent"
                ) || target.provider == "codex" =>
            {
                if value.is_empty() || headers.iter().any(|header| header.0 == name) {
                    return Err(invalid("invalid provider HTTP header"));
                }
                headers.push((name, value.to_owned()));
            }
            "host" | "user-agent" => {}
            "transfer-encoding"
            | "upgrade"
            | "proxy-authorization"
            | "forwarded"
            | "te"
            | "proxy-connection"
            | "connection"
            | "expect" => {
                return Err(invalid("unsupported provider HTTP header"));
            }
            _ => return Err(invalid("unsupported provider HTTP header")),
        }
    }
    let length = length.ok_or_else(|| invalid("missing provider HTTP content length"))?;
    Ok((headers, length, expect))
}

pub(super) fn read_line(input: &mut impl BufRead) -> io::Result<String> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(HEADER_LINE_MAX + 3).unwrap_or(u64::MAX);
    let read = io::Read::take(input, limit).read_until(b'\n', &mut bytes)?;
    if read == 0 || bytes.len() > HEADER_LINE_MAX + 2 || !bytes.ends_with(b"\r\n") {
        return Err(invalid("invalid provider HTTP line"));
    }
    bytes.truncate(bytes.len() - 2);
    if bytes
        .iter()
        .any(|byte| *byte == 0 || (*byte < b' ' && *byte != b'\t') || *byte == 0x7f)
    {
        return Err(invalid("invalid provider HTTP line"));
    }
    String::from_utf8(bytes).map_err(|_error| invalid("provider HTTP line is not UTF-8"))
}
