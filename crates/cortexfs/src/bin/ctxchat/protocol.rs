use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use serde_json::{Value, json};

pub(crate) fn request(socket: &Path, value: &Value) -> io::Result<Vec<Value>> {
    let mut stream = UnixStream::connect(socket)?;
    serde_json::to_writer(&mut stream, value)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut frames = Vec::new();
    for line in BufReader::new(stream).lines() {
        let line = line?;
        let frame = serde_json::from_str::<Value>(&line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let terminal = frame.get("type").and_then(Value::as_str) == Some("done");
        frames.push(frame);
        if terminal {
            break;
        }
    }
    Ok(frames)
}

pub(crate) fn send(socket: &Path, id: &str, session: &str, input: &str) -> io::Result<Vec<Value>> {
    request(
        socket,
        &json!({"op":"send", "id":id, "session":session, "input":input}),
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
    )
}
