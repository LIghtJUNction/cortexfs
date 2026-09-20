use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

pub(in crate::channel) fn server<const N: usize>(
    prefix: &str,
    responses: [&'static str; N],
) -> std::io::Result<(String, std::thread::JoinHandle<()>)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = format!("http://{}{prefix}", listener.local_addr()?);
    let server = std::thread::spawn(move || {
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
        if reader.read_until(b'\n', &mut line)? == 0 || line == b"\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix(b"Content-Length: ") {
            length = String::from_utf8_lossy(value).trim().parse().unwrap_or(0);
        }
    }
    reader.read_exact(&mut vec![0_u8; length])
}

fn parse_raw(request: &str) -> std::io::Result<super::HttpRequest> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    TcpStream::connect(listener.local_addr()?)?.write_all(request.as_bytes())?;
    let (mut server, _) = listener.accept()?;
    super::read_request(&mut server, 1024)
}

#[test]
fn content_length_framing_fails_closed() {
    assert!(parse_raw("POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length:+0\r\n\r\n").is_err());
    assert!(parse_raw("POST / HTTP/1.1\r\nContent-Length:0\r\nContent-Length:0\r\n\r\n").is_err());
    assert!(parse_raw("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n").is_err());
    assert!(parse_raw("POST / HTTP/1.1\r\nContent-Length : 0\r\n\r\n").is_err());
    assert!(parse_raw("POST / HTTP/1.1\r\nContent-Length\t: 0\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.1\r\nBad@Header: x\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.1 extra\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/2.0\r\n\r\n").is_err());
    assert!(parse_raw("GE@T / HTTP/1.1\r\n\r\n").is_err());
    assert!(parse_raw("GET /foo\tbar HTTP/1.1\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.1\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.1\r\nHost: one\r\nHost: two\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.0\r\n\r\n").is_ok());
    assert!(parse_raw("POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n").is_ok());
}
