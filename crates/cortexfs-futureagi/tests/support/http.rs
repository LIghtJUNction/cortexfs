use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

use serde_json::{Value, json};

#[expect(
    clippy::redundant_pub_crate,
    reason = "the sibling test module consumes this private fixture"
)]
pub(crate) type MockServer = (SocketAddr, Receiver<Value>, JoinHandle<io::Result<()>>);

#[expect(
    clippy::redundant_pub_crate,
    reason = "the sibling test module consumes this private fixture"
)]
pub(crate) fn start() -> io::Result<MockServer> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel();
    let server = thread::spawn(move || serve(listener, sender));
    Ok((address, receiver, server))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the thread entry owns its listener and sender"
)]
fn serve(listener: TcpListener, sender: mpsc::Sender<Value>) -> io::Result<()> {
    let (mut first, _) = listener.accept()?;
    let (request_line, _) = read_request(&mut first)?;
    if !request_line.starts_with("GET /sdk/api/v1/get-evals/") {
        return Err(io::Error::other("registry request was not GET"));
    }
    respond(
        &mut first,
        &json!({"result":[{"name":"answer_relevancy","config":{"required_keys":["input","output"]}}]}),
    )?;
    let (mut second, _) = listener.accept()?;
    let (request_line, body) = read_request(&mut second)?;
    if !request_line.starts_with("POST /sdk/api/v1/new-eval/") {
        return Err(io::Error::other("evaluation request was not POST"));
    }
    sender
        .send(body)
        .map_err(|error| io::Error::other(error.to_string()))?;
    respond(&mut second, &json!({"result":[]}))
}

fn read_request(stream: &mut TcpStream) -> io::Result<(String, Value)> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        extend(stream, &mut chunk, &mut bytes, "request ended")?;
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = String::from_utf8_lossy(
        bytes
            .get(..header_end)
            .ok_or_else(|| io::Error::other("missing request headers"))?,
    )
    .into_owned();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while bytes.len() < header_end + length {
        extend(stream, &mut chunk, &mut bytes, "body ended")?;
    }
    let request_line = headers.lines().next().unwrap_or_default().to_owned();
    let body = bytes
        .get(header_end..header_end + length)
        .filter(|slice| !slice.is_empty())
        .map(serde_json::from_slice)
        .transpose()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .unwrap_or(Value::Null);
    Ok((request_line, body))
}

fn extend(
    stream: &mut TcpStream,
    chunk: &mut [u8],
    bytes: &mut Vec<u8>,
    message: &str,
) -> io::Result<()> {
    let count = stream.read(chunk)?;
    if count == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, message));
    }
    bytes.extend_from_slice(
        chunk
            .get(..count)
            .ok_or_else(|| io::Error::other("invalid request chunk"))?,
    );
    Ok(())
}

fn respond(stream: &mut TcpStream, body: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(body).map_err(|error| io::Error::other(error.to_string()))?;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}
