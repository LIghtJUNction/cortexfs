#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;

    use cortexfs_runtime_client::{RuntimeClientError, status};

    #[test]
    fn queries_typed_session_status() -> Result<(), Box<dyn std::error::Error>> {
        for (kind, session) in [
            ("status", "default"),
            ("done", "default"),
            ("status", "other"),
        ] {
            let root = tempfile::tempdir()?;
            let socket = root.path().join("agent.sock");
            let listener = UnixListener::bind(&socket)?;
            let server = thread::spawn(move || -> Result<(), std::io::Error> {
                let (mut stream, _) = listener.accept()?;
                let mut frame = String::new();
                BufReader::new(&mut stream).read_line(&mut frame)?;
                assert_eq!(frame, "{\"op\":\"status\",\"session\":\"default\"}\n");
                writeln!(
                    stream,
                    "{}",
                    serde_json::json!({
                        "type": kind, "session": session, "status": "active", "step": 2,
                    })
                )
            });
            let result = status::status(&socket, "default");
            server
                .join()
                .map_err(|error| format!("server panicked: {error:?}"))??;
            if kind == "status" && session == "default" {
                let response = result?;
                assert_eq!((response.status.as_str(), response.step), ("active", 2));
            } else {
                assert_eq!(result, Err(RuntimeClientError::InvalidFrame));
            }
        }
        Ok(())
    }

    #[test]
    fn rejects_invalid_status_session_before_connecting() {
        for session in [String::new(), "\0".to_owned(), "x".repeat(256 * 1024)] {
            assert_eq!(
                status::status(std::path::Path::new("/missing.sock"), &session),
                Err(RuntimeClientError::InvalidRequest)
            );
        }
    }
}
