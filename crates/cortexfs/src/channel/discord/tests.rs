use std::{
    fmt::Write as FmtWrite,
    io::{BufRead, BufReader, Read, Write as IoWrite},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::Duration,
};

use cortexfs_channels::{ChannelId, ChannelProgressPolicy, ConversationId, MessageTarget};

use super::DiscordConfig;

mod commands;
mod message;
mod other;
mod security;
mod upload;

pub(super) struct Reply {
    pub status: &'static str,
    pub headers: &'static [(&'static str, &'static str)],
    pub body: &'static str,
}

pub(super) struct Request {
    pub head: String,
    pub body: Vec<u8>,
}

pub(super) fn server<const N: usize>(
    replies: [Reply; N],
) -> std::io::Result<(String, mpsc::Receiver<Request>, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let base = format!("http://{}", listener.local_addr()?);
    let (sender, requests) = mpsc::sync_channel(N);
    let worker = thread::spawn(move || {
        for reply in replies {
            let Ok((mut stream, _address)) = listener.accept() else {
                return;
            };
            let Ok(request) = read(&stream) else {
                return;
            };
            if sender.send(request).is_err() {
                return;
            }
            let mut headers = String::new();
            for &(name, value) in reply.headers {
                if write!(headers, "{name}: {value}\r\n").is_err() {
                    return;
                }
            }
            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{}",
                reply.status,
                reply.body.len(),
                reply.body
            );
            let _ignored = stream.write_all(response.as_bytes());
        }
    });
    Ok((base, requests, worker))
}

fn read(stream: &TcpStream) -> std::io::Result<Request> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut head = String::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
        head.push_str(&line);
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Request { head, body })
}

pub(super) fn config(api_base: String) -> DiscordConfig {
    DiscordConfig {
        allowed_senders: Vec::new(),
        application_id: "app".to_owned(),
        bot_token: "secret-token".to_owned(),
        agent_socket: "/tmp/unused-agent.sock".into(),
        agent: "executor".to_owned(),
        session_prefix: "discord".to_owned(),
        cwd: None,
        channel: None,
        api_base,
        gateway_url: "ws://127.0.0.1/unused".to_owned(),
        intents: 0,
        progress: ChannelProgressPolicy::default(),
    }
}

pub(super) fn target() -> Result<MessageTarget, cortexfs_channels::ChannelError> {
    Ok(MessageTarget {
        channel: ChannelId::from_static("discord"),
        conversation: ConversationId::new("123")?,
        thread: None,
        reply_to: None,
    })
}

pub(super) fn receive(
    receiver: &mpsc::Receiver<Request>,
) -> Result<Request, mpsc::RecvTimeoutError> {
    receiver.recv_timeout(Duration::from_secs(2))
}

pub(super) fn join(worker: thread::JoinHandle<()>) -> std::io::Result<()> {
    worker
        .join()
        .map_err(|_panic| std::io::Error::other("mock server panicked"))
}
