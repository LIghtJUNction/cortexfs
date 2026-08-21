use crate::*;
use std::fmt::Write as _;

pub(crate) fn read_client_token(stream: &mut UnixStream) -> io::Result<String> {
    let mut token = Vec::new();
    let mut byte = [0; 1];
    while token.len() < CLIENT_TOKEN_LIMIT {
        if stream.read(&mut byte)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ctxterm token must end with newline",
            ));
        }
        if byte[0] == b'\n' {
            break;
        }
        token.push(byte[0]);
    }
    let token = String::from_utf8(token)
        .map_err(|_error| io::Error::new(io::ErrorKind::InvalidInput, "invalid ctxterm token"))?;
    valid_client_token(&token)
        .then_some(token)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid ctxterm token"))
}

pub(crate) fn valid_client_token(token: &str) -> bool {
    token.len() >= CLIENT_TOKEN_MIN
        && token.len() < CLIENT_TOKEN_LIMIT
        && !token.contains(['\n', '\r'])
}

pub(crate) fn tokens_equal(expected: &[u8], supplied: &[u8]) -> bool {
    let mut difference = expected.len() ^ supplied.len();
    for index in 0..expected.len().max(supplied.len()) {
        difference |= usize::from(
            expected.get(index).copied().unwrap_or(0) ^ supplied.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

pub(crate) fn token_hash(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ignored = write!(output, "{byte:02x}");
            output
        })
}
