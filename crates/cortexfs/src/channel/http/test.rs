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
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)
}

fn parse_raw(request: &str) -> std::io::Result<super::HttpRequest> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let mut client = TcpStream::connect(listener.local_addr()?)?;
    client.write_all(request.as_bytes())?;
    let (mut server, _) = listener.accept()?;
    super::read_request(&mut server, 1024)
}

#[test]
fn content_length_framing_is_strict() {
    assert!(parse_raw("POST / HTTP/1.1\r\nContent-Length: +2\r\n\r\n{}").is_err());
    assert!(parse_raw("POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n{}").is_err());
    assert_eq!(
        parse_raw("POST / HTTP/1.1\r\nContent-Length: 02\r\n\r\n{}")?.body,
        "{}"
    );
    Ok::<_, std::io::Error>(())
}
