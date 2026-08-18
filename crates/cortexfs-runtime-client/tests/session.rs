use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::thread;

use cortexfs_runtime_client::{RuntimeClientError, SessionSendRequest, session};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_canonical_session_frame_and_collects_events() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || -> Result<(), std::io::Error> {
            let (mut stream, _) = listener.accept()?;
            let mut frame = String::new();
            BufReader::new(&mut stream).read_line(&mut frame)?;
            if !frame.contains("\"op\":\"send\"") || !frame.contains("\"scope\":\"private\"") {
                return Err(std::io::Error::other("unexpected session frame"));
            }
            stream.write_all(b"{\"type\":\"done\",\"status\":\"ok\"}\n")
        });
        let events = session::send(
            &socket,
            SessionSendRequest {
                request_id: "im-1",
                session: "im-abc",
                scope: "private",
                cwd: None,
                workspace: None,
                input: "hello",
            },
        )?;
        server
            .join()
            .map_err(|error| format!("server panicked: {error:?}"))??;
        assert_eq!(events, vec![r#"{"type":"done","status":"ok"}"#.to_owned()]);
        Ok(())
    }

    #[test]
    fn rejects_invalid_session_scope_before_connecting() {
        let result = session::send(
            std::path::Path::new("/missing.sock"),
            SessionSendRequest {
                request_id: "id",
                session: "session",
                scope: "unknown",
                cwd: None,
                workspace: None,
                input: "hello",
            },
        );
        assert_eq!(result, Err(RuntimeClientError::InvalidRequest));
    }
}
