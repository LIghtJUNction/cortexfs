#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs;
    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixListener;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

    fn write_frame(writer: &mut impl Write, value: &Value) -> TestResult<()> {
        writeln!(writer, "{value}")?;
        writer.flush()?;
        Ok(())
    }

    fn serve_approvals(listener: &UnixListener) -> TestResult<(String, String, String)> {
        listener.set_nonblocking(true)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let (mut stream, _address) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error)
                    if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    return Err(std::io::Error::new(
                        ErrorKind::TimedOut,
                        "ctx did not connect to approval socket",
                    )
                    .into());
                }
                Err(error) => return Err(error.into()),
            }
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut reader = BufReader::new(stream.try_clone()?);

        let mut request = String::new();
        reader.read_line(&mut request)?;
        write_frame(
            &mut stream,
            &json!({
                "type": "approval_request",
                "run": "run-1",
                "id": "call-1",
                "name": "example.echo",
                "args": ["one"]
            }),
        )?;
        let mut allowed = String::new();
        reader.read_line(&mut allowed)?;

        write_frame(
            &mut stream,
            &json!({
                "type": "approval_result",
                "run": "run-1",
                "id": "call-1",
                "name": "example.echo",
                "decision": "allow_once",
                "reason": "approved once"
            }),
        )?;
        write_frame(
            &mut stream,
            &json!({
                "type": "approval_request",
                "run": "run-1",
                "id": "call-2",
                "name": "other.tool",
                "args": []
            }),
        )?;
        let mut denied = String::new();
        reader.read_line(&mut denied)?;

        write_frame(
            &mut stream,
            &json!({
                "type": "approval_result",
                "run": "run-1",
                "id": "call-2",
                "name": "other.tool",
                "decision": "deny",
                "reason": "tool approval denied"
            }),
        )?;
        write_frame(
            &mut stream,
            &json!({
                "type": "message",
                "run": "run-1",
                "role": "assistant",
                "content": [{ "type": "text", "text": "approved" }]
            }),
        )?;
        write_frame(
            &mut stream,
            &json!({ "type": "done", "run": "run-1", "status": "ok" }),
        )?;
        stream.shutdown(Shutdown::Write)?;
        Ok((request, allowed, denied))
    }

    #[test]
    fn ctx_agent_send_approves_only_explicit_tool() -> TestResult<()> {
        let root = tempfile::tempdir()?;
        let agent_dir = root.path().join("agent");
        fs::create_dir_all(&agent_dir)?;
        let listener = UnixListener::bind(agent_dir.join("reviewer.sock"))?;
        let server = std::thread::spawn(move || serve_approvals(&listener));

        let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
            .arg("--root")
            .arg(root.path())
            .args([
                "agent",
                "send",
                "reviewer",
                "--session",
                "default",
                "--approve",
                "example.echo",
                "go",
            ])
            .env("NO_COLOR", "1")
            .output()?;
        let (request, allowed, denied) = server
            .join()
            .map_err(|_panic| std::io::Error::other("approval server thread panicked"))??;

        assert!(
            output.status.success(),
            "ctx agent send failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"approved\n");
        assert!(output.stderr.is_empty());

        let request: Value = serde_json::from_str(&request)?;
        assert_eq!(request.get("op"), Some(&json!("send")));
        assert_eq!(request.get("session"), Some(&json!("default")));
        assert_eq!(request.get("input"), Some(&json!("go")));
        assert_eq!(
            serde_json::from_str::<Value>(&allowed)?,
            json!({
                "op": "approve",
                "run": "run-1",
                "id": "call-1",
                "decision": "allow_once"
            })
        );
        assert_eq!(
            serde_json::from_str::<Value>(&denied)?,
            json!({
                "op": "approve",
                "run": "run-1",
                "id": "call-2",
                "decision": "deny"
            })
        );
        Ok(())
    }
}
