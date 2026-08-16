use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use serde_json::{Value, json};

const MAX_FRAME_BYTES: usize = cortexfs::MAX_SOCKET_FRAME_BYTES;
const MAX_RESPONSE_BYTES: usize = MAX_FRAME_BYTES * 4;
const MAX_FRAMES: usize = 8192;

pub(crate) fn request(
    socket: &Path,
    value: &Value,
    approvals: &[String],
) -> io::Result<Vec<Value>> {
    let mut stream = UnixStream::connect(socket)?;
    serde_json::to_writer(&mut stream, value)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut frames = Vec::new();
    let mut response_bytes = 0usize;
    let mut saw_error = false;
    let mut reader = BufReader::new(stream.try_clone()?);
    while let Some(line) = read_frame(&mut reader)? {
        response_bytes = response_bytes
            .checked_add(line.len().saturating_add(1))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "response exceeds limit"))?;
        if response_bytes > MAX_RESPONSE_BYTES || frames.len() >= MAX_FRAMES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response exceeds limit",
            ));
        }
        let frame = serde_json::from_str::<Value>(&line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if frame.get("type").and_then(Value::as_str) == Some("approval_request") {
            respond_approval(&mut stream, &frame, approvals)?;
        }
        saw_error |= frame.get("type").and_then(Value::as_str) == Some("error");
        let done = frame.get("type").and_then(Value::as_str) == Some("done");
        frames.push(frame);
        if done {
            return Ok(frames);
        }
    }
    if saw_error {
        return Ok(frames);
    }
    Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "socket closed before done frame",
    ))
}

fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_FRAME_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let read = reader.by_ref().take(limit).read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > MAX_FRAME_BYTES || bytes.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response frame exceeds limit",
        ));
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_error| io::Error::new(io::ErrorKind::InvalidData, "response frame is not UTF-8"))
}

fn respond_approval(
    stream: &mut UnixStream,
    frame: &Value,
    approvals: &[String],
) -> io::Result<()> {
    let object = frame
        .as_object()
        .filter(|object| object.len() == 5)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid approval request"))?;
    let field = |name| object.get(name).and_then(Value::as_str);
    let run = field("run")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid approval run"))?;
    let id = field("id")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid approval id"))?;
    let name = field("name")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid approval name"))?;
    if frame.get("args").and_then(Value::as_array).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid approval request",
        ));
    }
    let decision = if approvals.iter().any(|allowed| allowed == name) {
        "allow_once"
    } else {
        "deny"
    };
    serde_json::to_writer(
        &mut *stream,
        &json!({"op":"approve", "run":run, "id":id, "decision":decision}),
    )?;
    stream.write_all(b"\n")?;
    stream.flush()
}

pub(crate) fn send(
    socket: &Path,
    id: &str,
    session: &str,
    input: &str,
    approvals: &[String],
) -> io::Result<Vec<Value>> {
    request(
        socket,
        &json!({"op":"send", "id":id, "session":session, "input":input}),
        approvals,
    )
}

pub(crate) fn tsh(
    socket: &Path,
    id: &str,
    session: &str,
    args: &[String],
) -> io::Result<Vec<Value>> {
    request(
        socket,
        &json!({"op":"tsh", "id":id, "session":session, "args":args}),
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[test]
    fn approval_round_trip_keeps_socket_writable() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || -> io::Result<Value> {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            stream.write_all(b"{\"type\":\"approval_request\",\"run\":\"r1\",\"id\":\"c1\",\"name\":\"example.echo\",\"args\":[]}\n")?;
            stream.flush()?;
            line.clear();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            stream.write_all(b"{\"type\":\"done\"}\n")?;
            serde_json::from_str(line.trim_end()).map_err(io::Error::other)
        });
        let frames = send(
            &socket,
            "r1",
            "default",
            "hello",
            &["example.echo".to_owned()],
        )?;
        assert_eq!(
            frames
                .last()
                .and_then(|v| v.get("type"))
                .and_then(Value::as_str),
            Some("done")
        );
        let response = server
            .join()
            .map_err(|_panic| io::Error::other("server panicked"))??;
        assert_eq!(
            response.get("decision").and_then(Value::as_str),
            Some("allow_once")
        );
        Ok(())
    }

    #[test]
    fn eof_without_done_is_rejected() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            stream.write_all(b"{\"type\":\"delta\",\"text\":\"partial\"}\n")
        });
        let error = request(&socket, &json!({"op":"ping"}), &[])
            .err()
            .ok_or_else(|| io::Error::other("missing EOF error"))?;
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        server
            .join()
            .map_err(|_panic| io::Error::other("server panicked"))??;
        Ok(())
    }

    #[test]
    fn eof_after_error_preserves_error_frames() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            stream.write_all(b"{\"type\":\"error\",\"code\":\"EIO\"}\n")
        });
        let frames = request(&socket, &json!({"op":"send"}), &[])?;
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames
                .first()
                .and_then(|frame| frame.get("type"))
                .and_then(Value::as_str),
            Some("error")
        );
        server
            .join()
            .map_err(|_panic| io::Error::other("server panicked"))??;
        Ok(())
    }

    #[test]
    fn oversized_frame_without_newline_is_rejected_during_read() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            stream.write_all(&vec![b'x'; MAX_FRAME_BYTES.saturating_add(1)])
        });
        let error = request(&socket, &json!({"op":"ping"}), &[])
            .err()
            .ok_or_else(|| io::Error::other("missing frame limit error"))?;
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        server
            .join()
            .map_err(|_panic| io::Error::other("server panicked"))??;
        Ok(())
    }
}
