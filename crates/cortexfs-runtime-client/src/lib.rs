pub mod agent;
pub mod interaction;
pub mod session;
pub mod status;
pub use session::SessionSendRequest;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_FRAME_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RuntimeClientError {
    #[error("invalid runtime environment")]
    InvalidEnvironment,
    #[error("invalid runtime request")]
    InvalidRequest,
    #[error("cannot connect to runtime socket")]
    CannotConnect,
    #[error("cannot write runtime request")]
    CannotWrite,
    #[error("cannot read runtime response")]
    CannotRead,
    #[error("invalid runtime frame")]
    InvalidFrame,
    #[error("runtime rejected request: {0}")]
    Rejected(String),
}

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
        /// Deprecated compatibility input; ignored and omitted by new frames.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        token: String,
        request_id: String,
        agent: String,
        session: String,
        run: String,
    },
    #[serde(rename = "agent.create")]
    CreateChild {
        /// Deprecated compatibility input; ignored and omitted by new frames.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        token: String,
        request_id: String,
        agent: String,
        session: String,
        run: String,
        child: String,
        child_session: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(
            default,
            deserialize_with = "deserialize_present_u32",
            skip_serializing_if = "Option::is_none"
        )]
        window: Option<u32>,
        input: String,
        life: String,
    },
    #[serde(rename = "agent.update")]
    UpdatePrompt {
        /// Deprecated compatibility input; ignored and omitted by new frames.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        token: String,
        request_id: String,
        agent: String,
        session: String,
        run: String,
        control: String,
        content: String,
    },
}

/// Maximum accepted `agent.update` prompt-control payload in bytes.
pub const MAX_SELF_UPDATE_CONTENT_BYTES: usize = 8 * 1024;

/// Returns whether an authority-free prompt control may be self-updated.
#[must_use]
pub fn is_agent_prompt_control(name: &str) -> bool {
    matches!(name, "system.md" | "prompt.template.md")
}

fn deserialize_present_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    u32::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateChildResult {
    pub child: String,
    pub child_session: String,
    pub pid: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreateChildEnvironmentRequest<'a> {
    pub request_id: &'a str,
    pub child: &'a str,
    pub child_session: &'a str,
    pub path: Option<&'a str>,
    pub window: Option<u32>,
    pub input: &'a str,
    pub life: &'a str,
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
    #[serde(rename = "agent.updated")]
    PromptUpdated { request_id: String },
}

impl RequestFrame {
    #[must_use]
    pub fn request_id(&self) -> &str {
        match *self {
            Self::Ping { ref request_id, .. }
            | Self::CreateChild { ref request_id, .. }
            | Self::UpdatePrompt { ref request_id, .. } => request_id,
        }
    }
}

pub(crate) fn read_frame<T: DeserializeOwned>(
    stream: UnixStream,
    max: u64,
    timeout: Duration,
) -> Result<T, RuntimeClientError> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    let mut bytes = Vec::new();
    BufReader::new(stream)
        .take(max + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    if bytes.is_empty()
        || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max
        || bytes.last() != Some(&b'\n')
    {
        return Err(RuntimeClientError::InvalidFrame);
    }
    serde_json::from_slice(&bytes).map_err(|_error| RuntimeClientError::InvalidFrame)
}

pub fn request(socket: &Path, frame: &RequestFrame) -> Result<ResponseFrame, RuntimeClientError> {
    let mut stream =
        UnixStream::connect(socket).map_err(|_error| RuntimeClientError::CannotConnect)?;
    serde_json::to_writer(&mut stream, frame).map_err(|_error| RuntimeClientError::CannotWrite)?;
    stream
        .write_all(b"\n")
        .map_err(|_error| RuntimeClientError::CannotWrite)?;
    let response = read_frame(
        stream,
        MAX_FRAME_BYTES,
        if cfg!(test) {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(5)
        },
    )?;
    let response_id = match response {
        ResponseFrame::Pong { ref request_id, .. }
        | ResponseFrame::Error { ref request_id, .. }
        | ResponseFrame::ChildCreated { ref request_id, .. }
        | ResponseFrame::PromptUpdated { ref request_id } => request_id,
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
            ) | (
                RequestFrame::UpdatePrompt { .. },
                ResponseFrame::PromptUpdated { .. } | ResponseFrame::Error { .. }
            )
        )
    {
        return Err(RuntimeClientError::InvalidFrame);
    }
    Ok(response)
}

pub fn ping(
    socket: &Path,
    _token: &str,
    request_id: &str,
    agent: &str,
    session: &str,
    run: &str,
) -> Result<Option<RuntimeSourceReceipt>, RuntimeClientError> {
    match request(
        socket,
        &RequestFrame::Ping {
            token: String::new(),
            request_id: request_id.to_owned(),
            agent: agent.to_owned(),
            session: session.to_owned(),
            run: run.to_owned(),
        },
    )? {
        ResponseFrame::Pong { receipt, .. } => Ok(receipt),
        ResponseFrame::Error { errno, .. } => Err(RuntimeClientError::Rejected(errno)),
        ResponseFrame::ChildCreated { .. } | ResponseFrame::PromptUpdated { .. } => {
            Err(RuntimeClientError::InvalidFrame)
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "wire helper keeps capability and parent identity fields explicit"
)]
pub fn create_child(
    socket: &Path,
    _token: &str,
    request_id: &str,
    agent: &str,
    session: &str,
    run: &str,
    child: &str,
    child_session: &str,
    path: Option<&str>,
    window: Option<u32>,
    input: &str,
    life: &str,
) -> Result<CreateChildResult, RuntimeClientError> {
    if window == Some(0) {
        return Err(RuntimeClientError::InvalidEnvironment);
    }
    match request(
        socket,
        &RequestFrame::CreateChild {
            token: String::new(),
            request_id: request_id.to_owned(),
            agent: agent.to_owned(),
            session: session.to_owned(),
            run: run.to_owned(),
            child: child.to_owned(),
            child_session: child_session.to_owned(),
            path: path.map(str::to_owned),
            window,
            input: input.to_owned(),
            life: life.to_owned(),
        },
    )? {
        ResponseFrame::ChildCreated { result, .. } => Ok(result),
        ResponseFrame::Error { errno, .. } => Err(RuntimeClientError::Rejected(errno)),
        ResponseFrame::Pong { .. } | ResponseFrame::PromptUpdated { .. } => {
            Err(RuntimeClientError::InvalidFrame)
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "wire helper keeps capability and self-update fields explicit"
)]
pub fn update_prompt(
    socket: &Path,
    _token: &str,
    request_id: &str,
    agent: &str,
    session: &str,
    run: &str,
    control: &str,
    content: &str,
) -> Result<(), RuntimeClientError> {
    if !is_agent_prompt_control(control) || content.len() > MAX_SELF_UPDATE_CONTENT_BYTES {
        return Err(RuntimeClientError::InvalidEnvironment);
    }
    match request(
        socket,
        &RequestFrame::UpdatePrompt {
            token: String::new(),
            request_id: request_id.to_owned(),
            agent: agent.to_owned(),
            session: session.to_owned(),
            run: run.to_owned(),
            control: control.to_owned(),
            content: content.to_owned(),
        },
    )? {
        ResponseFrame::PromptUpdated { .. } => Ok(()),
        ResponseFrame::Error { errno, .. } => Err(RuntimeClientError::Rejected(errno)),
        ResponseFrame::Pong { .. } | ResponseFrame::ChildCreated { .. } => {
            Err(RuntimeClientError::InvalidFrame)
        }
    }
}

/// Environment-derived request for one self prompt-control update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdatePromptEnvironmentRequest<'a> {
    pub request_id: &'a str,
    pub control: &'a str,
    pub content: &'a str,
}

pub fn update_prompt_from_environment(
    request: UpdatePromptEnvironmentRequest<'_>,
) -> Result<(), RuntimeClientError> {
    let socket = env::var_os("CTX_CONTROL_SOCKET").ok_or(RuntimeClientError::InvalidEnvironment)?;
    let agent = env::var("CTX_AGENT").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let session =
        env::var("CTX_SESSION").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let run = env::var("CTX_RUN_ID").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    update_prompt(
        &PathBuf::from(socket),
        "",
        request.request_id,
        &agent,
        &session,
        &run,
        request.control,
        request.content,
    )
}

pub fn create_child_from_environment(
    request: CreateChildEnvironmentRequest<'_>,
) -> Result<CreateChildResult, RuntimeClientError> {
    if request.window == Some(0) {
        return Err(RuntimeClientError::InvalidEnvironment);
    }
    let socket = env::var_os("CTX_CONTROL_SOCKET").ok_or(RuntimeClientError::InvalidEnvironment)?;
    let agent = env::var("CTX_AGENT").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let session =
        env::var("CTX_SESSION").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let run = env::var("CTX_RUN_ID").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    create_child(
        &PathBuf::from(socket),
        "",
        request.request_id,
        &agent,
        &session,
        &run,
        request.child,
        request.child_session,
        request.path,
        request.window,
        request.input,
        request.life,
    )
}

pub fn ping_from_environment(
    agent: &str,
) -> Result<Option<RuntimeSourceReceipt>, RuntimeClientError> {
    let Some(socket) = env::var_os("CTX_CONTROL_SOCKET") else {
        return Ok(None);
    };
    let session =
        env::var("CTX_SESSION").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let run = env::var("CTX_RUN_ID").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let step = env::var("CTX_AGENT_STEP")
        .ok()
        .and_then(|value| value.parse::<u8>().ok());
    ping(
        &PathBuf::from(socket),
        "",
        &startup_ping_request_id(&run, step),
        agent,
        &session,
        &run,
    )
}

fn startup_ping_request_id(run: &str, step: Option<u8>) -> String {
    match step {
        Some(step) if step > 0 => format!("startup-{run}-{step}"),
        _ => format!("startup-{run}"),
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

    type TestServer = (
        tempfile::TempDir,
        PathBuf,
        thread::JoinHandle<Option<RequestFrame>>,
    );

    fn server(response: Vec<u8>) -> Result<TestServer, RuntimeClientError> {
        let root = tempfile::tempdir().map_err(|_error| RuntimeClientError::CannotConnect)?;
        let socket = root.path().join("control.sock");
        let listener =
            UnixListener::bind(&socket).map_err(|_error| RuntimeClientError::CannotConnect)?;
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().ok()?;
            let mut bytes = Vec::new();
            BufReader::new(&mut stream)
                .read_until(b'\n', &mut bytes)
                .ok()?;
            let frame = serde_json::from_slice(&bytes).ok()?;
            let _ignored = stream.write_all(&response);
            Some(frame)
        });
        Ok((root, socket, server))
    }

    fn response(bytes: Vec<u8>) -> Result<ResponseFrame, RuntimeClientError> {
        let (_root, socket, server) = server(bytes)?;
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
    fn create_child_response_has_exact_parity() -> Result<(), RuntimeClientError> {
        let (_root, socket, server) = server(
            b"{\"type\":\"agent.created\",\"request_id\":\"request-1\",\"result\":{\"child\":\"c\",\"child_session\":\"s\",\"pid\":42}}\n".to_vec(),
        )?;
        let result = create_child(
            &socket,
            "token",
            "request-1",
            "agent",
            "session",
            "run",
            "c",
            "s",
            Some("/ctx/home/1000/tool"),
            Some(2048),
            "input",
            "temp",
        );
        assert!(matches!(
            server.join(),
            Ok(Some(RequestFrame::CreateChild { life, path, .. }))
                if life == "temp" && path.as_deref() == Some("/ctx/home/1000/tool")
        ));
        assert_eq!(
            result,
            Ok(CreateChildResult {
                child: "c".to_owned(),
                child_session: "s".to_owned(),
                pid: 42
            })
        );
        Ok(())
    }

    #[test]
    fn create_child_window_wire_is_numeric_optional_and_strict()
    -> Result<(), Box<dyn std::error::Error>> {
        let frame = RequestFrame::CreateChild {
            token: "token".to_owned(),
            request_id: "request-1".to_owned(),
            agent: "agent".to_owned(),
            session: "session".to_owned(),
            run: "run".to_owned(),
            child: "child".to_owned(),
            child_session: "child-session".to_owned(),
            path: None,
            window: Some(2048),
            input: "work".to_owned(),
            life: "owned".to_owned(),
        };
        let encoded = serde_json::to_value(&frame)?;
        assert_eq!(
            encoded.get("window").and_then(serde_json::Value::as_u64),
            Some(2048)
        );
        let mut absent = encoded;
        if let Some(object) = absent.as_object_mut() {
            object.remove("window");
        }
        assert!(matches!(
            serde_json::from_value::<RequestFrame>(absent),
            Ok(RequestFrame::CreateChild { window: None, .. })
        ));
        let mut omitted = frame.clone();
        if let RequestFrame::CreateChild { ref mut window, .. } = omitted {
            *window = None;
        }
        assert!(serde_json::to_value(omitted)?.get("window").is_none());
        for value in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("1"),
            serde_json::Value::Null,
            serde_json::json!(4_294_967_296_u64),
        ] {
            let mut invalid = serde_json::to_value(&frame)?;
            invalid
                .as_object_mut()
                .ok_or("serialized create child request should be an object")?
                .insert("window".to_owned(), value);
            assert!(serde_json::from_value::<RequestFrame>(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn update_prompt_response_has_exact_parity() -> Result<(), RuntimeClientError> {
        let (_root, socket, server) =
            server(b"{\"type\":\"agent.updated\",\"request_id\":\"request-1\"}\n".to_vec())?;
        let result = update_prompt(
            &socket,
            "token",
            "request-1",
            "agent",
            "session",
            "run",
            "system.md",
            "iterate\n",
        );
        assert!(matches!(
            server.join(),
            Ok(Some(RequestFrame::UpdatePrompt {
                control, content, ..
            })) if control == "system.md" && content == "iterate\n"
        ));
        assert_eq!(result, Ok(()));
        Ok(())
    }

    #[test]
    fn illegal_update_prompt_fails_before_connect() {
        for (control, content) in [
            ("policy", "allow".to_owned()),
            ("window", "auto\n".to_owned()),
            ("system.md", "x".repeat(MAX_SELF_UPDATE_CONTENT_BYTES + 1)),
        ] {
            assert_eq!(
                update_prompt(
                    Path::new("/definitely/missing.sock"),
                    "token",
                    "request-1",
                    "agent",
                    "session",
                    "run",
                    control,
                    &content,
                ),
                Err(RuntimeClientError::InvalidEnvironment)
            );
        }
    }

    #[test]
    fn zero_window_fails_before_connect() {
        assert_eq!(
            create_child(
                Path::new("/definitely/missing.sock"),
                "token",
                "request-1",
                "agent",
                "session",
                "run",
                "child",
                "child-session",
                None,
                Some(0),
                "work",
                "owned",
            ),
            Err(RuntimeClientError::InvalidEnvironment)
        );
    }

    #[test]
    #[ignore = "subprocess entrypoint for environment isolation"]
    fn socketless_environment_is_inert_subprocess() {
        assert_eq!(ping_from_environment("agent"), Ok(None));
    }

    #[test]
    fn socketless_environment_is_inert() -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new(env::current_exe()?)
            .arg("--exact")
            .arg("tests::socketless_environment_is_inert_subprocess")
            .arg("--ignored")
            .env_remove("CTX_CONTROL_SOCKET")
            .status()?;
        assert!(status.success());
        Ok(())
    }

    #[test]
    fn tokenless_frames_omit_token_and_legacy_token_is_accepted() -> Result<(), serde_json::Error> {
        let tokenless = serde_json::to_value(RequestFrame::Ping {
            token: String::new(),
            request_id: "request-1".to_owned(),
            agent: "agent".to_owned(),
            session: "session".to_owned(),
            run: "run".to_owned(),
        })?;
        assert!(tokenless.get("token").is_none());
        let legacy = serde_json::from_value::<RequestFrame>(serde_json::json!({
            "op": "ping",
            "token": "legacy",
            "request_id": "request-1",
            "agent": "agent",
            "session": "session",
            "run": "run"
        }))?;
        assert!(matches!(legacy, RequestFrame::Ping { token, .. } if token == "legacy"));
        Ok(())
    }

    #[test]
    fn continuation_startup_ping_ids_are_unique() {
        assert_eq!(startup_ping_request_id("run-1", Some(0)), "startup-run-1");
        assert_eq!(startup_ping_request_id("run-1", Some(1)), "startup-run-1-1");
        assert_eq!(startup_ping_request_id("run-1", None), "startup-run-1");
    }
}
