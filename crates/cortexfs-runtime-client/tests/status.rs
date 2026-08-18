#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;

    use cortexfs_runtime_client::{RuntimeClientError, status};

    #[test]
    fn queries_typed_session_status() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || -> Result<(), std::io::Error> {
            let (mut stream, _) = listener.accept()?;
            let mut frame = String::new();
            BufReader::new(&mut stream).read_line(&mut frame)?;
            if frame != "{\"op\":\"status\",\"session\":\"default\"}\n" {
                return Err(std::io::Error::other("unexpected status frame"));
            }
            stream.write_all(
                b"{\"type\":\"status\",\"session\":\"default\",\"status\":\"active\",\"step\":2}\n",
            )
        });
        let result = status::status(&socket, "default")?;
        server
            .join()
            .map_err(|error| format!("server panicked: {error:?}"))??;
        assert_eq!(result.status, "active");
        assert_eq!(result.step, 2);
        Ok(())
    }

    #[test]
    fn rejects_empty_status_session_before_connecting() {
        assert_eq!(
            status::status(std::path::Path::new("/missing.sock"), ""),
            Err(RuntimeClientError::InvalidRequest)
        );
    }
}
