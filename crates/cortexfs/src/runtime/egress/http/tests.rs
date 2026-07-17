use std::io::{BufReader, Cursor, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::*;

fn target(base: &str) -> ProviderTarget {
    provider_target("fixture", base)
}

fn provider_target(provider: &str, base: &str) -> ProviderTarget {
    ProviderTarget {
        provider: provider.to_owned(),
        base_url: format!("http://example.test{base}"),
        authority: "http://example.test".to_owned(),
        base_path: base.to_owned(),
    }
}

#[test]
fn parser_forwards_codex_metadata_only_for_codex_target() -> io::Result<()> {
    let input = b"POST /backend-api/codex/responses HTTP/1.1\r\nAuthorization: Bearer access\r\nChatGPT-Account-Id: account\r\nOriginator: ctx\r\nSession-Id: run-1\r\nUser-Agent: cortexfs/test\r\nContent-Length: 2\r\n\r\n{}";
    let mut output = Vec::new();
    let request = parse_request(
        &mut BufReader::new(Cursor::new(input)),
        &mut output,
        &provider_target("codex", "/backend-api/codex"),
    )?;
    assert_eq!(
        request
            .headers
            .iter()
            .map(|header| header.0.as_str())
            .collect::<Vec<_>>(),
        [
            "authorization",
            "chatgpt-account-id",
            "originator",
            "session-id",
            "user-agent"
        ]
    );
    assert!(
        parse_request(
            &mut BufReader::new(Cursor::new(input)),
            &mut Vec::new(),
            &provider_target("fixture", "/backend-api/codex")
        )
        .is_err()
    );
    Ok(())
}

fn parse(input: &[u8], base: &str) -> io::Result<(Request, Vec<u8>)> {
    let mut output = Vec::new();
    let request = parse_request(
        &mut BufReader::new(Cursor::new(input)),
        &mut output,
        &target(base),
    )?;
    Ok((request, output))
}

#[test]
fn parser_accepts_allowlist_and_expect_then_reads_exact_body() -> io::Result<()> {
    let input = b"POST /v1/messages HTTP/1.1\r\nHost: attacker.test\r\nAuthorization: Bearer secret\r\nAnthropic-Version: 2023-06-01\r\nContent-Type: application/json\r\nExpect: 100-continue\r\nContent-Length: 2\r\n\r\n{}tail";
    let (request, output) = parse(input, "/v1")?;
    assert_eq!(request.endpoint, "messages");
    assert_eq!(request.body, b"{}");
    assert_eq!(request.headers.len(), 3);
    assert_eq!(output, b"HTTP/1.1 100 Continue\r\n\r\n");
    Ok(())
}

#[test]
fn parser_rejects_request_target_and_protocol_injection() {
    for input in [
        b"GET /v1/responses HTTP/1.1\r\nContent-Length: 0\r\n\r\n".as_slice(),
        b"CONNECT example.test:443 HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        b"POST http://evil/v1/responses HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        b"POST /v1/../responses HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        b"POST /v1/responses?q=1 HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        b"POST /v1/responses HTTP/2\r\nContent-Length: 0\r\n\r\n",
    ] {
        assert!(parse(input, "/v1").is_err());
    }
}

#[test]
fn parser_rejects_framing_smuggling_and_unapproved_headers() {
    for headers in [
        "Transfer-Encoding: chunked\r\n",
        "Content-Length: 0\r\nContent-Length: 0\r\n",
        "Upgrade: websocket\r\nContent-Length: 0\r\n",
        "Forwarded: host=evil\r\nContent-Length: 0\r\n",
        "X-Evil: value\r\nContent-Length: 0\r\n",
        "Content-Length: 0\r\n folded: yes\r\n",
        "Connection:\r\nContent-Length: 0\r\n",
        "Connection: authorization\r\nAuthorization: Bearer secret\r\nContent-Length: 0\r\n",
        "Connection: keep-alive, authorization\r\nAuthorization: Bearer secret\r\nContent-Length: 0\r\n",
        "cOnNeCtIoN: keep-alive\r\nContent-Length: 0\r\n",
        "Connection: keep-alive\r\nConnection: close\r\nContent-Length: 0\r\n",
    ] {
        let input = format!("POST /responses HTTP/1.1\r\n{headers}\r\n");
        assert!(parse(input.as_bytes(), "").is_err());
    }
}

#[test]
fn monitor_error_reaps_curl_group_and_closes_upstream_within_one_second()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let accepted = Arc::new(AtomicBool::new(false));
    let did_accept = Arc::clone(&accepted);
    let upstream = thread::spawn(move || -> io::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut request = [0; 4096];
        let _read = stream.read(&mut request)?;
        did_accept.store(true, Ordering::Release);
        let mut closed = [0; 1];
        stream.read_exact(&mut closed)
    });
    let target = ProviderTarget {
        provider: "fixture".to_owned(),
        base_url: format!("http://{address}/v1"),
        authority: format!("http://{address}"),
        base_path: "/v1".to_owned(),
    };
    let (_client, server) = UnixStream::pair()?;
    let monitor_fd = server.as_raw_fd();
    MONITOR_ERROR_FD.store(monitor_fd, Ordering::Release);
    MONITOR_ERROR_PID.store(0, Ordering::Release);
    let request = Request {
        endpoint: "responses",
        headers: Vec::new(),
        body: b"{}".to_vec(),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let relay = thread::spawn(move || run_curl(server, &target, &request, &stop));
    let deadline = Instant::now() + Duration::from_secs(1);
    while !accepted.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(accepted.load(Ordering::Acquire));
    let pid = MONITOR_ERROR_PID.load(Ordering::Acquire);
    assert_ne!(pid, 0);
    MONITOR_ERROR_ARMED.store(true, Ordering::Release);
    assert!(relay.join().map_err(|_panic| "relay panicked")?.is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    let pid = i32::try_from(pid)?;
    assert_eq!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
        Err(nix::errno::Errno::ESRCH)
    );
    assert!(
        upstream
            .join()
            .map_err(|_panic| "upstream panicked")?
            .is_err()
    );
    MONITOR_ERROR_FD.store(-1, Ordering::Release);
    Ok(())
}

#[test]
fn parser_enforces_line_count_total_and_body_bounds() {
    let long = format!(
        "POST /responses HTTP/1.1\r\nAccept: {}\r\nContent-Length: 0\r\n\r\n",
        "x".repeat(HEADER_LINE_MAX)
    );
    assert!(parse(long.as_bytes(), "").is_err());
    let many = format!(
        "POST /responses HTTP/1.1\r\n{}Content-Length: 0\r\n\r\n",
        "Host: x\r\n".repeat(HEADER_COUNT_MAX)
    );
    assert!(parse(many.as_bytes(), "").is_err());
    let huge = format!(
        "POST /responses HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        BODY_MAX + 1
    );
    assert!(parse(huge.as_bytes(), "").is_err());
}

#[test]
fn curl_config_preserves_utf8_and_keeps_secrets_out_of_process_arguments() -> io::Result<()> {
    let request = Request {
        endpoint: "responses",
        headers: vec![("authorization".to_owned(), "Bearer secret".to_owned())],
        body: "{\"input\":\"你好\"}".as_bytes().to_vec(),
    };
    let config = curl_config(&target("/v1"), &request)?;
    assert!(config.contains("Bearer secret"));
    assert!(config.contains("你好"));
    assert!(!config.contains("location = true"));
    Ok(())
}

#[test]
fn silent_upstream_is_killed_when_client_disconnects() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let upstream = thread::spawn(move || -> io::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut request = [0; 4096];
        let _read = stream.read(&mut request)?;
        let mut closed = [0; 1];
        stream.read_exact(&mut closed)
    });
    let target = ProviderTarget {
        provider: "fixture".to_owned(),
        base_url: format!("http://{address}/v1"),
        authority: format!("http://{address}"),
        base_path: "/v1".to_owned(),
    };
    let (client, server) = UnixStream::pair()?;
    let request = Request {
        endpoint: "responses",
        headers: Vec::new(),
        body: b"{}".to_vec(),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let relay = thread::spawn(move || run_curl(server, &target, &request, &stop));
    thread::sleep(Duration::from_millis(50));
    drop(client);
    let _result = relay.join().map_err(|_panic| "relay panicked")?;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(
        upstream
            .join()
            .map_err(|_panic| "upstream panicked")?
            .is_err()
    );
    Ok(())
}

#[test]
fn curl_raw_response_preserves_chunked_sse_and_non_success_status()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, body) in [
        ("200 OK", "data: ok\n\n"),
        ("429 Too Many Requests", "{\"error\":true}"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let expected = body.to_owned();
        let upstream = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let mut request = [0; 4096];
            let read = stream.read(&mut request)?;
            let bytes = request
                .get(..read)
                .ok_or_else(|| io::Error::other("invalid request range"))?;
            let text = String::from_utf8_lossy(bytes);
            assert!(text.starts_with("POST /v1/responses HTTP/1.1\r\n"));
            assert!(text.contains("Host: "));
            assert!(!text.contains("attacker.test"));
            write!(
                stream,
                "HTTP/1.1 {status}\r\nTransfer-Encoding: chunked\r\nContent-Type: text/event-stream\r\n\r\n{:x}\r\n{expected}\r\n0\r\n\r\n",
                expected.len()
            )
        });
        let target = ProviderTarget {
            provider: "fixture".to_owned(),
            base_url: format!("http://{address}/v1"),
            authority: format!("http://{address}"),
            base_path: "/v1".to_owned(),
        };
        let (mut client, server) = UnixStream::pair()?;
        let request = Request {
            endpoint: "responses",
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            body: b"{}".to_vec(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let relay = thread::spawn(move || run_curl(server, &target, &request, &stop));
        let mut response = Vec::new();
        client.read_to_end(&mut response)?;
        relay.join().map_err(|_panic| "relay panicked")??;
        upstream.join().map_err(|_panic| "upstream panicked")??;
        let text = String::from_utf8(response)?;
        assert!(text.starts_with(&format!("HTTP/1.1 {status}\r\n")));
        assert!(text.contains("Transfer-Encoding: chunked\r\n"));
        assert!(text.contains(&format!("{:x}\r\n{body}\r\n0\r\n\r\n", body.len())));
    }
    Ok(())
}
