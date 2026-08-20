use crate::*;

const MAX_SOCKET_RUNTIME_SMALL_FILE_BYTES: u64 = 64 * 1024;
const MAX_SOCKET_RUNTIME_EVENTS_BYTES: u64 = 1024 * 1024;
const MAX_SOCKET_RUNTIME_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_AGENT_EXECUTABLE_FRAME_BYTES: usize = 256 * 1024;
const MAX_AGENT_EXECUTABLE_STDERR_BYTES: u64 = 64 * 1024;
const SOCKET_REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Handles one JSONL socket request frame against a durable session root.
///
/// This is the reusable core for a future Unix socket loop: it parses one
/// request, applies `CortexFS` session-file semantics, and returns canonical
/// response frames. It does not call a model provider and does not execute
/// tools.
pub fn handle_socket_request_frame(
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
    frame: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let request = parse_socket_request_frame(frame).map_err(SocketRuntimeError::Request)?;
    handle_socket_request(session_root, default_cwd, model, &request)
}

/// Handles one parsed socket request against a durable session root.
pub fn handle_socket_request(
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
    request: &SocketRequest,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    match *request {
        SocketRequest::Ping => Ok(SocketRuntimeResponse::new(vec![socket_pong_frame()])),
        SocketRequest::Send { .. } => {
            handle_socket_send(session_root, default_cwd, model, request, None).map(|outcome| {
                match outcome {
                    SocketSendOutcome::Recorded(response)
                    | SocketSendOutcome::Replayed(response) => response,
                }
            })
        }
        SocketRequest::Resume {
            ref session,
            ref after,
        } => handle_socket_resume(session_root, session, after.as_deref()),
        SocketRequest::Status { ref session } => handle_socket_status(session_root, model, session),
        SocketRequest::Cancel { ref id } => handle_socket_cancel(session_root, id),
        SocketRequest::Tsh { .. } | SocketRequest::Stop { .. } => Err(SocketRuntimeError::Record(
            SocketSessionRecordError::UnsupportedRequest,
        )),
    }
}

/// Builds a canonical socket error response frame from a runtime error.
#[must_use]
pub fn socket_runtime_error_response(error: &SocketRuntimeError) -> SocketRuntimeResponse {
    SocketRuntimeResponse::new(vec![
        serde_json::json!({
            "type": "error",
            "code": error.errno(),
            "message": error.errno()
        })
        .to_string(),
    ])
}

/// Accepts and serves one Unix socket connection.
///
/// This is a bounded runtime helper for `name.sock` implementations. It does
/// not loop, spawn, supervise, or watch files; callers decide process lifetime.
pub fn serve_unix_socket_listener_once(
    listener: &UnixListener,
    peer_policy: Option<SocketPeerPolicy>,
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let (mut stream, _addr) = listener
        .accept()
        .map_err(|_error| SocketRuntimeError::CannotAcceptConnection)?;
    serve_unix_socket_stream_once(&mut stream, peer_policy, session_root, default_cwd, model)
}

/// Accepts one Unix socket connection and dispatches `send` to an agent executable.
///
/// This is the reference socket-activated agent runtime path. It preserves the
/// durable socket request semantics, then runs the ABI executable object for
/// `send` requests and returns its canonical JSONL events to the client.
pub fn serve_agent_executable_socket_listener_once(
    listener: &UnixListener,
    peer_policy: Option<SocketPeerPolicy>,
    runtime: AgentExecutableSocketRuntime<'_>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    serve_agent_executable_socket_listener_once_with_stop(listener, peer_policy, runtime, None)
}

/// Accepts one agent connection with an optional privileged stop handler.
pub fn serve_agent_executable_socket_listener_once_with_stop(
    listener: &UnixListener,
    peer_policy: Option<SocketPeerPolicy>,
    runtime: AgentExecutableSocketRuntime<'_>,
    stop: Option<&dyn AgentStopHandler>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let (mut stream, _addr) = listener
        .accept()
        .map_err(|_error| SocketRuntimeError::CannotAcceptConnection)?;
    serve_agent_executable_socket_stream_once_with_stop(&mut stream, peer_policy, runtime, stop)
}

/// Serves one connected stream and dispatches `send` to an agent executable.
pub fn serve_agent_executable_socket_stream_once(
    stream: &mut UnixStream,
    peer_policy: Option<SocketPeerPolicy>,
    runtime: AgentExecutableSocketRuntime<'_>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    serve_agent_executable_socket_stream_once_with_stop(stream, peer_policy, runtime, None)
}

/// Serves one connected agent stream with an optional privileged stop handler.
pub fn serve_agent_executable_socket_stream_once_with_stop(
    stream: &mut UnixStream,
    peer_policy: Option<SocketPeerPolicy>,
    runtime: AgentExecutableSocketRuntime<'_>,
    stop: Option<&dyn AgentStopHandler>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    serve_socket_stream_with(stream, peer_policy, |stream, frame| {
        let peer_uid = peer_credentials(stream)
            .map_err(SocketRuntimeError::PeerCredential)?
            .uid();
        handle_agent_executable_socket_request_frame_streaming(
            stream, runtime, stop, peer_uid, frame,
        )
    })
}

/// Serves one connected Unix socket stream request.
///
/// This helper enforces optional kernel peer credentials before reading a
/// single JSONL frame, then writes either the request response or a stable
/// error frame. It is intentionally one-shot; process supervision and accept
/// loops remain outside the ABI.
pub fn serve_unix_socket_stream_once(
    stream: &mut UnixStream,
    peer_policy: Option<SocketPeerPolicy>,
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    serve_socket_stream_with(stream, peer_policy, |stream, frame| {
        let response = handle_socket_request_frame(session_root, default_cwd, model, frame)?;
        write_socket_runtime_response(stream, &response)?;
        Ok(response)
    })
}

pub(crate) fn serve_socket_stream_with(
    stream: &mut UnixStream,
    peer_policy: Option<SocketPeerPolicy>,
    dispatch: impl FnOnce(&mut UnixStream, &str) -> Result<SocketRuntimeResponse, SocketRuntimeError>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    if let Some(policy) = peer_policy {
        let peer = peer_credentials(stream).map_err(SocketRuntimeError::PeerCredential)?;
        if !policy.allows(peer) {
            let error = SocketRuntimeError::PeerDenied;
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            return Err(error);
        }
    }

    let frame = match read_socket_request_frame_from_stream(stream) {
        Ok(frame) => frame,
        Err(error) => {
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            return Err(error);
        }
    };
    match dispatch(stream, &frame) {
        Ok(response) => Ok(response),
        Err(error @ SocketRuntimeError::PostAcceptStop) => Err(error),
        Err(error) => {
            let response = socket_runtime_error_response(&error);
            let _ignored = write_socket_runtime_response(stream, &response);
            Err(error)
        }
    }
}

pub mod bwrap;
pub mod events;
pub mod exec;
pub mod session;
pub mod status;
pub mod stream;

pub(crate) use bwrap::*;
pub(crate) use events::*;
pub(crate) use exec::*;
pub(crate) use session::*;
pub(crate) use status::*;
pub(crate) use stream::*;

#[cfg(test)]
mod stop_tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::sync::{Arc, Mutex};

    struct StopHandler {
        calls: Arc<Mutex<Vec<&'static str>>>,
        peer_uids: Arc<Mutex<Vec<u32>>>,
        client: Mutex<Option<UnixStream>>,
        fail: bool,
    }

    struct Prepared {
        calls: Arc<Mutex<Vec<&'static str>>>,
        client: Option<UnixStream>,
        fail: bool,
    }

    impl AgentStopHandler for StopHandler {
        fn preflight(
            &self,
            _agent: &str,
            peer_uid: u32,
        ) -> Result<Box<dyn PreparedAgentStop>, SocketRuntimeError> {
            self.calls
                .lock()
                .map_err(|_error| SocketRuntimeError::CannotRunAgent)?
                .push("preflight");
            self.peer_uids
                .lock()
                .map_err(|_error| SocketRuntimeError::CannotRunAgent)?
                .push(peer_uid);
            Ok(Box::new(Prepared {
                calls: Arc::clone(&self.calls),
                client: self
                    .client
                    .lock()
                    .map_err(|_error| SocketRuntimeError::CannotRunAgent)?
                    .take(),
                fail: self.fail,
            }))
        }
    }

    impl PreparedAgentStop for Prepared {
        fn execute(mut self: Box<Self>) -> Result<(), SocketRuntimeError> {
            if let Some(client) = self.client.as_mut() {
                let mut response = String::new();
                BufReader::new(client)
                    .read_line(&mut response)
                    .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
                assert!(response.contains("\"type\":\"accepted\""));
            }
            self.calls
                .lock()
                .map_err(|_error| SocketRuntimeError::CannotRunAgent)?
                .push("execute");
            if self.fail {
                Err(SocketRuntimeError::CannotRunAgent)
            } else {
                Ok(())
            }
        }
    }

    fn runtime(identity: &AgentUnixIdentity) -> AgentExecutableSocketRuntime<'_> {
        AgentExecutableSocketRuntime {
            ctx_root: Path::new("/source"),
            source_root: Path::new("/source"),
            identity,
            env: &[],
            session_root: Path::new("/session"),
            default_cwd: "/",
            model: None,
            network_allowed: false,
            agent_name: "parent",
            agent_executable: Path::new("/agent"),
            environment: RunEnvironment::Native,
        }
    }

    #[test]
    fn stop_flushes_accepted_before_synchronous_execute() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut client, mut server) = UnixStream::pair()?;
        let observer = client.try_clone()?;
        client.write_all(b"{\"op\":\"stop\",\"agent\":\"parent\"}\n")?;
        client.shutdown(std::net::Shutdown::Write)?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let peer_uids = Arc::new(Mutex::new(Vec::new()));
        let handler = StopHandler {
            calls: Arc::clone(&calls),
            peer_uids: Arc::clone(&peer_uids),
            client: Mutex::new(Some(observer)),
            fail: false,
        };
        let identity = AgentUnixIdentity::new(1000, 1000, []);
        let result = serve_agent_executable_socket_stream_once_with_stop(
            &mut server,
            None,
            runtime(&identity),
            Some(&handler),
        );
        assert!(result.is_ok());
        assert!(
            calls
                .lock()
                .is_ok_and(|calls| *calls == ["preflight", "execute"])
        );
        assert!(
            peer_uids
                .lock()
                .is_ok_and(|peer_uids| *peer_uids == [nix::unistd::geteuid().as_raw()])
        );
        Ok(())
    }

    #[test]
    fn stop_rejects_wrong_runtime_agent_before_preflight() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut client, mut server) = UnixStream::pair()?;
        client.write_all(b"{\"op\":\"stop\",\"agent\":\"child\"}\n")?;
        client.shutdown(std::net::Shutdown::Write)?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let handler = StopHandler {
            calls: Arc::clone(&calls),
            peer_uids: Arc::new(Mutex::new(Vec::new())),
            client: Mutex::new(None),
            fail: false,
        };
        let identity = AgentUnixIdentity::new(1000, 1000, []);
        assert_eq!(
            serve_agent_executable_socket_stream_once_with_stop(
                &mut server,
                None,
                runtime(&identity),
                Some(&handler),
            ),
            Err(SocketRuntimeError::PeerDenied)
        );
        assert!(calls.lock().is_ok_and(|calls| calls.is_empty()));
        Ok(())
    }

    #[test]
    fn stop_execution_failure_after_accept_does_not_write_second_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut client, mut server) = UnixStream::pair()?;
        client.write_all(b"{\"op\":\"stop\",\"agent\":\"parent\"}\n")?;
        client.shutdown(std::net::Shutdown::Write)?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let handler = StopHandler {
            calls,
            peer_uids: Arc::new(Mutex::new(Vec::new())),
            client: Mutex::new(None),
            fail: true,
        };
        let identity = AgentUnixIdentity::new(1000, 1000, []);
        assert_eq!(
            serve_agent_executable_socket_stream_once_with_stop(
                &mut server,
                None,
                runtime(&identity),
                Some(&handler),
            ),
            Err(SocketRuntimeError::PostAcceptStop)
        );
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response)?;
        assert_eq!(response.lines().count(), 1);
        assert!(response.contains("\"type\":\"accepted\""));
        Ok(())
    }
}
