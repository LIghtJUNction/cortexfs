use std::io::Write;
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
            if super::read_request(&mut stream, super::MAX_HTTP_BODY_BYTES).is_err() {
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

fn parse_raw(request: &str) -> std::io::Result<super::HttpRequest> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    TcpStream::connect(listener.local_addr()?)?.write_all(request.as_bytes())?;
    let (mut server, _) = listener.accept()?;
    super::read_request(&mut server, 1024)
}

#[test]
fn content_length_framing_fails_closed() -> std::io::Result<()> {
    assert!(parse_raw("POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length:+0\r\n\r\n").is_err());
    assert!(parse_raw("POST / HTTP/1.1\r\nContent-Length:0\r\nContent-Length:0\r\n\r\n").is_err());
    assert!(parse_raw("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n").is_err());
    assert!(parse_raw("POST / HTTP/1.1\r\nContent-Length : 0\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.1\r\nHost:x\r\nX-Line-Signature:a\0b\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.1\r\nBad@Header: x\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.1 extra\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/2.0\r\n\r\n").is_err());
    assert!(parse_raw("GE@T / HTTP/1.1\r\n\r\n").is_err());
    assert!(parse_raw("GET /foo\tbar HTTP/1.1\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.1\r\n\r\n").is_err());
    assert!(parse_raw("GET / HTTP/1.1\r\nHost: one\r\nHost: two\r\n\r\n").is_err());
    assert!(
        parse_raw("GET / HTTP/1.1\r\nHost:x\r\nX-Line-Signature:a\r\nx-line-signature:b\r\n\r\n")
            .is_err()
    );
    let r = parse_raw("GET / HTTP/1.0\r\nX-Test: \t\u{a0}b\u{a0}\t \r\n\r\n")?;
    assert!(r.headers.get("x-test").is_some_and(|v| v == "\u{a0}b\u{a0}"));
    assert!(parse_raw("POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n").is_ok());
    Ok(())
}
