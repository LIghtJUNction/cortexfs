//! Narrow client and shared wire protocol for a runtime capability socket.

use serde::{Deserialize, Serialize};
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_FRAME_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeClientError {
    InvalidEnvironment,
    CannotConnect,
    CannotWrite,
    CannotRead,
    InvalidFrame,
    Rejected(String),
}

/// Generates a legal request id with 128 bits of operating-system randomness.
pub fn fresh_request_id(prefix: &str) -> Result<String, RuntimeClientError> {
    if prefix.is_empty()
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || prefix.len().saturating_add(33) > 128
    {
        return Err(RuntimeClientError::InvalidEnvironment);
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_error| RuntimeClientError::CannotRead)?;
    let mut id = String::with_capacity(prefix.len() + 33);
    id.push_str(prefix);
    id.push('-');
    for byte in random {
        use std::fmt::Write as _;
        write!(id, "{byte:02x}").map_err(|_error| RuntimeClientError::CannotWrite)?;
    }
    Ok(id)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum RequestFrame {
    #[serde(rename = "ping")]
    Ping {
        token: String,
        request_id: String,
        agent: String,
        session: String,
        run: String,
    },
    #[serde(rename = "agent.create")]
    CreateChild {
        token: String,
        request_id: String,
        agent: String,
        session: String,
        run: String,
        child: String,
        child_session: String,
        input: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateChildResult {
    pub child: String,
    pub child_session: String,
    pub pid: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSourceReceipt {
    pub path: String,
    pub dev: u64,
    pub ino: u64,
    pub kind: RuntimeSourceKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeSourceKind {
    PlainDirectory,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ResponseFrame {
    #[serde(rename = "pong")]
    Pong {
        request_id: String,
        receipt: Option<RuntimeSourceReceipt>,
    },
    #[serde(rename = "error")]
    Error { request_id: String, errno: String },
    #[serde(rename = "agent.created")]
    ChildCreated {
        request_id: String,
        result: CreateChildResult,
    },
}

impl RequestFrame {
    #[must_use]
    pub fn request_id(&self) -> &str {
        match *self {
            Self::Ping { ref request_id, .. } | Self::CreateChild { ref request_id, .. } => {
                request_id
            }
        }
    }
}

pub fn request(socket: &Path, frame: &RequestFrame) -> Result<ResponseFrame, RuntimeClientError> {
    let mut stream =
        UnixStream::connect(socket).map_err(|_error| RuntimeClientError::CannotConnect)?;
    serde_json::to_writer(&mut stream, frame).map_err(|_error| RuntimeClientError::CannotWrite)?;
    stream
        .write_all(b"\n")
        .map_err(|_error| RuntimeClientError::CannotWrite)?;
    stream
        .set_read_timeout(Some(if cfg!(test) {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(5)
        }))
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    let mut bytes = Vec::new();
    BufReader::new(stream)
        .take(MAX_FRAME_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    if bytes.is_empty()
        || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FRAME_BYTES
        || bytes.last() != Some(&b'\n')
    {
        return Err(RuntimeClientError::InvalidFrame);
    }
    let response: ResponseFrame =
        serde_json::from_slice(&bytes).map_err(|_error| RuntimeClientError::InvalidFrame)?;
    let response_id = match response {
        ResponseFrame::Pong { ref request_id, .. }
        | ResponseFrame::Error { ref request_id, .. }
        | ResponseFrame::ChildCreated { ref request_id, .. } => request_id,
    };
    if response_id != frame.request_id()
        || !matches!(
            (frame, &response),
            (
                RequestFrame::Ping { .. },
                ResponseFrame::Pong { .. } | ResponseFrame::Error { .. }
            ) | (
                RequestFrame::CreateChild { .. },
                ResponseFrame::ChildCreated { .. } | ResponseFrame::Error { .. }
            )
        )
    {
        return Err(RuntimeClientError::InvalidFrame);
    }
    Ok(response)
}

pub fn ping(
    socket: &Path,
    token: &str,
    request_id: &str,
    agent: &str,
    session: &str,
    run: &str,
) -> Result<Option<RuntimeSourceReceipt>, RuntimeClientError> {
    match request(
        socket,
        &RequestFrame::Ping {
            token: token.to_owned(),
            request_id: request_id.to_owned(),
            agent: agent.to_owned(),
            session: session.to_owned(),
            run: run.to_owned(),
        },
    )? {
        ResponseFrame::Pong { receipt, .. } => Ok(receipt),
        ResponseFrame::Error { errno, .. } => Err(RuntimeClientError::Rejected(errno)),
        ResponseFrame::ChildCreated { .. } => Err(RuntimeClientError::InvalidFrame),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "wire helper keeps capability and parent identity fields explicit"
)]
pub fn create_child(
    socket: &Path,
    token: &str,
    request_id: &str,
    agent: &str,
    session: &str,
    run: &str,
    child: &str,
    child_session: &str,
    input: &str,
) -> Result<CreateChildResult, RuntimeClientError> {
    match request(
        socket,
        &RequestFrame::CreateChild {
            token: token.to_owned(),
            request_id: request_id.to_owned(),
            agent: agent.to_owned(),
            session: session.to_owned(),
            run: run.to_owned(),
            child: child.to_owned(),
            child_session: child_session.to_owned(),
            input: input.to_owned(),
        },
    )? {
        ResponseFrame::ChildCreated { result, .. } => Ok(result),
        ResponseFrame::Error { errno, .. } => Err(RuntimeClientError::Rejected(errno)),
        ResponseFrame::Pong { .. } => Err(RuntimeClientError::InvalidFrame),
    }
}

pub fn create_child_from_environment(
    request_id: &str,
    child: &str,
    child_session: &str,
    input: &str,
) -> Result<CreateChildResult, RuntimeClientError> {
    let socket = env::var_os("CTX_CONTROL_SOCKET").ok_or(RuntimeClientError::InvalidEnvironment)?;
    let token =
        env::var("CTX_CONTROL_TOKEN").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let agent = env::var("CTX_AGENT").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let session =
        env::var("CTX_SESSION").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let run = env::var("CTX_RUN_ID").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    create_child(
        &PathBuf::from(socket),
        &token,
        request_id,
        &agent,
        &session,
        &run,
        child,
        child_session,
        input,
    )
}

pub fn ping_from_environment(
    agent: &str,
) -> Result<Option<RuntimeSourceReceipt>, RuntimeClientError> {
    let socket = env::var_os("CTX_CONTROL_SOCKET");
    let token = env::var("CTX_CONTROL_TOKEN").ok();
    match (socket, token) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(RuntimeClientError::InvalidEnvironment),
        (Some(socket), Some(token)) => {
            let session =
                env::var("CTX_SESSION").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
            let run =
                env::var("CTX_RUN_ID").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
            ping(
                &PathBuf::from(socket),
                &token,
                &format!("startup-{run}"),
                agent,
                &session,
                &run,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::process::Command;
    use std::thread;

    fn ping_frame() -> RequestFrame {
        RequestFrame::Ping {
            token: "token".to_owned(),
            request_id: "request-1".to_owned(),
            agent: "agent".to_owned(),
            session: "session".to_owned(),
            run: "run".to_owned(),
        }
    }

    fn response(bytes: Vec<u8>) -> Result<ResponseFrame, RuntimeClientError> {
        let root = tempfile::tempdir().map_err(|_error| RuntimeClientError::CannotConnect)?;
        let socket = root.path().join("control.sock");
        let listener =
            UnixListener::bind(&socket).map_err(|_error| RuntimeClientError::CannotConnect)?;
        let server = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request_bytes = Vec::new();
            let _ignored = BufReader::new(&mut stream).read_until(b'\n', &mut request_bytes);
            let _ignored = stream.write_all(&bytes);
        });
        let result = request(&socket, &ping_frame());
        let _ignored = server.join();
        result
    }

    #[test]
    fn rejects_wrong_id_and_response_type() {
        assert_eq!(
            response(b"{\"type\":\"pong\",\"request_id\":\"wrong\"}\n".to_vec()),
            Err(RuntimeClientError::InvalidFrame)
        );
        assert_eq!(response(b"{\"type\":\"agent.created\",\"request_id\":\"request-1\",\"result\":{\"child\":\"c\",\"child_session\":\"s\",\"pid\":1}}\n".to_vec()), Err(RuntimeClientError::InvalidFrame));
    }

    #[test]
    fn ping_returns_authoritative_source_receipt() {
        let response = response(b"{\"type\":\"pong\",\"request_id\":\"request-1\",\"receipt\":{\"path\":\"/source\",\"dev\":7,\"ino\":9,\"kind\":\"plain-directory\"}}\n".to_vec());
        assert_eq!(
            response,
            Ok(ResponseFrame::Pong {
                request_id: "request-1".to_owned(),
                receipt: Some(RuntimeSourceReceipt {
                    path: "/source".to_owned(),
                    dev: 7,
                    ino: 9,
                    kind: RuntimeSourceKind::PlainDirectory
                })
            })
        );
    }

    #[test]
    fn fresh_request_ids_are_legal_and_distinct() -> Result<(), RuntimeClientError> {
        let first = fresh_request_id("tsh-cache")?;
        let second = fresh_request_id("tsh-cache")?;
        assert_ne!(first, second);
        assert!(first.len() <= 128);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        );
        assert!(fresh_request_id("bad prefix").is_err());
        Ok(())
    }

    #[test]
    fn rejects_oversized_and_missing_newline() {
        let mut oversized = vec![b'x'; usize::try_from(MAX_FRAME_BYTES).unwrap_or(16_384)];
        oversized.push(b'\n');
        assert_eq!(response(oversized), Err(RuntimeClientError::InvalidFrame));
        assert_eq!(
            response(br#"{"type":"pong","request_id":"request-1"}"#.to_vec()),
            Err(RuntimeClientError::InvalidFrame)
        );
    }

    #[test]
    fn read_timeout_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("control.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || {
            if let Ok((_stream, _)) = listener.accept() {
                thread::sleep(Duration::from_millis(200));
            }
        });
        assert_eq!(
            request(&socket, &ping_frame()),
            Err(RuntimeClientError::CannotRead)
        );
        let _ignored = server.join();
        Ok(())
    }

    #[test]
    fn create_child_response_has_exact_parity() {
        let root = tempfile::tempdir().ok();
        assert!(root.is_some());
        let Some(root) = root else { return };
        let socket = root.path().join("control.sock");
        let listener = UnixListener::bind(&socket).ok();
        assert!(listener.is_some());
        let Some(listener) = listener else { return };
        let server = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut bytes = Vec::new();
            let _ignored = BufReader::new(&mut stream).read_until(b'\n', &mut bytes);
            let _ignored = stream.write_all(b"{\"type\":\"agent.created\",\"request_id\":\"request-1\",\"result\":{\"child\":\"c\",\"child_session\":\"s\",\"pid\":42}}\n");
        });
        let result = create_child(
            &socket,
            "token",
            "request-1",
            "agent",
            "session",
            "run",
            "c",
            "s",
            "input",
        );
        let _ignored = server.join();
        assert_eq!(
            result,
            Ok(CreateChildResult {
                child: "c".to_owned(),
                child_session: "s".to_owned(),
                pid: 42
            })
        );
    }

    #[test]
    #[ignore = "subprocess entrypoint for environment isolation"]
    fn partial_environment_subprocess() {
        assert_eq!(
            ping_from_environment("agent"),
            Err(RuntimeClientError::InvalidEnvironment)
        );
    }

    #[test]
    fn partial_environment_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new(env::current_exe()?)
            .arg("--exact")
            .arg("tests::partial_environment_subprocess")
            .arg("--ignored")
            .env("CTX_CONTROL_TOKEN", "partial")
            .env_remove("CTX_CONTROL_SOCKET")
            .status()?;
        assert!(status.success());
        Ok(())
    }
}
