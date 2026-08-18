use std::{
    collections::BTreeSet,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    time::Duration,
};

use super::*;

type TestResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn config(base: String) -> Config {
    Config {
        token: "token".to_owned(),
        api_base: base,
        allowed_users: BTreeSet::new(),
        socket: PathBuf::from("/run/cortexfs/channel/wechat.sock"),
        poll_timeout: Duration::from_secs(5),
        reply_timeout: Duration::from_secs(5),
        channel_version: "test".to_owned(),
        wechat_uin: "dW5pdA==".to_owned(),
    }
}

#[tokio::test]
async fn get_updates_uses_i_link_headers_and_cursor() -> TestResult<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || -> std::io::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let bytes = buffer
                .get(..count)
                .ok_or_else(|| std::io::Error::other("read count exceeds buffer"))?;
            request.extend_from_slice(bytes);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
        assert!(request.contains("/ilink/bot/getupdates"));
        assert!(request.contains("authorization: bearer token"));
        assert!(request.contains("x-wechat-uin: dw5pda=="));
        let body = r#"{"get_updates_buf":"next","msgs":[{"from_user_id":"u"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(())
    });
    let config = config(format!("http://{address}"));
    let batch = get_updates(&client(&config)?, &config, "old").await?;
    assert_eq!(batch.cursor, "next");
    assert_eq!(batch.messages.len(), 1);
    let server_result = server
        .join()
        .map_err(|error| std::io::Error::other(format!("server panicked: {error:?}")))?;
    server_result?;
    Ok(())
}
