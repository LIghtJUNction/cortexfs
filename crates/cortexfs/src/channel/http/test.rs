use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    thread,
};

#[expect(
    clippy::redundant_pub_crate,
    reason = "the mock server is shared by crate-local channel tests"
)]
pub(crate) fn server<const N: usize>(
    prefix: &str,
    responses: [&str; N],
) -> std::io::Result<(String, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = format!("http://{}{prefix}", listener.local_addr()?);
    let responses = responses.map(str::to_owned);
    let server = thread::spawn(move || {
        for body in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            if read_request(&stream).is_err() {
                return;
            }
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            if stream.write_all(reply.as_bytes()).is_err() {
                return;
            }
        }
    });
    Ok((address, server))
}

fn read_request(stream: &TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut length = 0_usize;
    loop {
        let mut line = Vec::new();
        reader.read_until(b'\n', &mut line)?;
        if line == b"\r\n" || line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix(b"Content-Length: ") {
            length = String::from_utf8_lossy(value).trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)
}

fn parse_raw(request: &str) -> std::io::Result<super::HttpRequest> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let request = request.as_bytes().to_vec();
    let client = thread::spawn(move || {
        let mut stream = TcpStream::connect(address).expect("connect test client");
        stream.write_all(&request).expect("write test request");
    });
    let (mut stream, _) = listener.accept()?;
    let result = super::read_request(&mut stream, 1024);
    client.join().expect("join test client");
    result
}

#[test]
fn content_length_uses_http_decimal_grammar() {
    assert!(parse_raw("POST / HTTP/1.1\r\nContent-Length: +2\r\n\r\n{}").is_err());
    let valid = parse_raw("POST / HTTP/1.1\r\nContent-Length: 02\r\n\r\n{}").expect("valid length");
    assert_eq!(valid.body, "{}");
}

#[test]
fn duplicate_content_length_fails_closed() {
    let request = "POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n{}";
    assert!(parse_raw(request).is_err());
}
