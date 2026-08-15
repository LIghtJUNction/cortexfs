use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::support::process::{read_limited_bytes, terminate_process_group};

use super::ProviderTarget;

const HEADER_LINE_MAX: usize = 8 * 1024;
const HEADER_TOTAL_MAX: usize = 32 * 1024;
const HEADER_COUNT_MAX: usize = 64;
const BODY_MAX: usize = 4 * 1024 * 1024;
const STDERR_MAX: usize = 16 * 1024;
const IO_PAUSE: Duration = Duration::from_millis(10);
const CLIENT_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Eq, PartialEq)]
struct Request {
    endpoint: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub(super) fn relay(
    mut local: UnixStream,
    target: &ProviderTarget,
    shutdown: &Arc<AtomicBool>,
) -> io::Result<()> {
    local.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    local.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    let mut input = BufReader::new(local.try_clone()?);
    let request = match parse_request(&mut input, &mut local, target) {
        Ok(request) => request,
        Err(error) => {
            let _ignored = local.write_all(
                b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            );
            return Err(error);
        }
    };
    if let Some(credential) = target.credential.as_ref() {
        let bearer = format!("Bearer {}", credential.token);
        let authorized = request.headers.iter().any(|header| {
            (header.0 == "authorization" && header.1 == bearer)
                || (header.0 == "x-api-key" && header.1 == credential.token)
        });
        if !authorized {
            let _ignored = local.write_all(
                b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            );
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid provider egress credential",
            ));
        }
    }
    let request = inject_provider_credential(request, target);
    run_curl(local, target, &request, shutdown)
}

fn parse_request(
    input: &mut impl BufRead,
    output: &mut impl Write,
    target: &ProviderTarget,
) -> io::Result<Request> {
    let request_line = read_line(input, HEADER_LINE_MAX)?;
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if parts.next().is_some() || method != "POST" || version != "HTTP/1.1" {
        return Err(invalid("unsupported provider HTTP request line"));
    }
    let endpoint = endpoint_for_path(path, &target.base_path)?;
    let mut total = request_line.len() + 2;
    let mut count = 0;
    let mut content_length = None;
    let mut expect_continue = false;
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let line = read_line(input, HEADER_LINE_MAX)?;
        total = total.saturating_add(line.len() + 2);
        if total > HEADER_TOTAL_MAX {
            return Err(invalid("provider HTTP headers exceed limit"));
        }
        if line.is_empty() {
            break;
        }
        count += 1;
        if count > HEADER_COUNT_MAX || line.starts_with([' ', '\t']) {
            return Err(invalid("invalid provider HTTP header"));
        }
        let (raw_name, raw_value) = line
            .split_once(':')
            .ok_or_else(|| invalid("invalid provider HTTP header"))?;
        if !valid_header_name(raw_name) || has_controls(raw_value) {
            return Err(invalid("invalid provider HTTP header"));
        }
        let name = raw_name.to_ascii_lowercase();
        let value = raw_value.trim_matches([' ', '\t']);
        match name.as_str() {
            "content-length" => {
                if content_length.is_some() || value.is_empty() {
                    return Err(invalid("duplicate provider HTTP content length"));
                }
                let length = value
                    .parse::<usize>()
                    .map_err(|_error| invalid("invalid provider HTTP content length"))?;
                if length > BODY_MAX {
                    return Err(invalid("provider HTTP body exceeds limit"));
                }
                content_length = Some(length);
            }
            "expect" if value.eq_ignore_ascii_case("100-continue") => {
                if expect_continue {
                    return Err(invalid("duplicate provider HTTP expectation"));
                }
                expect_continue = true;
            }
            "authorization" | "x-api-key" | "anthropic-version" | "content-type" | "accept"
            | "chatgpt-account-id" | "originator" | "session-id" | "user-agent"
                if !matches!(
                    name.as_str(),
                    "chatgpt-account-id" | "originator" | "session-id" | "user-agent"
                ) || target.provider == "codex" =>
            {
                if headers.iter().any(|header| header.0 == name) || value.is_empty() {
                    return Err(invalid("invalid provider HTTP header"));
                }
                headers.push((name, value.to_owned()));
            }
            "host" | "user-agent" => {}
            "transfer-encoding"
            | "upgrade"
            | "proxy-authorization"
            | "forwarded"
            | "te"
            | "proxy-connection"
            | "connection"
            | "expect" => {
                return Err(invalid("unsupported provider HTTP header"));
            }
            _ => return Err(invalid("unsupported provider HTTP header")),
        }
    }
    let length = content_length.ok_or_else(|| invalid("missing provider HTTP content length"))?;
    if expect_continue {
        output.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        output.flush()?;
    }
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    Ok(Request {
        endpoint,
        headers,
        body,
    })
}

fn inject_provider_credential(mut request: Request, target: &ProviderTarget) -> Request {
    let Some(credential) = target.credential.as_ref() else {
        return request;
    };
    request.headers.retain(|header| {
        !matches!(
            header.0.as_str(),
            "authorization"
                | "x-api-key"
                | "anthropic-version"
                | "chatgpt-account-id"
                | "originator"
                | "session-id"
                | "user-agent"
        )
    });
    if request.endpoint == "messages" {
        request.headers.extend([
            ("x-api-key".to_owned(), credential.token.clone()),
            ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
        ]);
        return request;
    }
    request.headers.push((
        "authorization".to_owned(),
        format!("Bearer {}", credential.token),
    ));
    if target.provider == "codex"
        && let Some(account_id) = credential.codex_account_id.as_deref()
    {
        request.headers.extend([
            ("chatgpt-account-id".to_owned(), account_id.to_owned()),
            ("originator".to_owned(), "ctx".to_owned()),
            ("session-id".to_owned(), credential.run.clone()),
            (
                "user-agent".to_owned(),
                format!("cortexfs/{}", env!("CARGO_PKG_VERSION")),
            ),
        ]);
    }
    request
}

fn endpoint_for_path(path: &str, base: &str) -> io::Result<&'static str> {
    if !path.starts_with('/')
        || path.contains(['?', '#', '\\'])
        || path.split('/').any(|p| p == "..")
    {
        return Err(invalid("invalid provider HTTP path"));
    }
    for endpoint in ["chat/completions", "responses", "messages"] {
        let expected = if base.is_empty() || base == "/" {
            format!("/{endpoint}")
        } else {
            format!("{base}/{endpoint}")
        };
        if path == expected {
            return Ok(endpoint);
        }
    }
    Err(invalid("unsupported provider HTTP path"))
}

fn read_line(input: &mut impl BufRead, limit: usize) -> io::Result<String> {
    let mut bytes = Vec::new();
    let read = input
        .take(u64::try_from(limit + 3).unwrap_or(u64::MAX))
        .read_until(b'\n', &mut bytes)?;
    if read == 0 || bytes.len() > limit + 2 || !bytes.ends_with(b"\r\n") {
        return Err(invalid("invalid provider HTTP line"));
    }
    bytes.truncate(bytes.len() - 2);
    if bytes
        .iter()
        .any(|byte| *byte == 0 || (*byte < b' ' && *byte != b'\t') || *byte == 0x7f)
    {
        return Err(invalid("invalid provider HTTP line"));
    }
    String::from_utf8(bytes).map_err(|_error| invalid("provider HTTP line is not UTF-8"))
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

fn has_controls(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| (byte < b' ' && byte != b'\t') || byte == 0x7f)
}

fn run_curl(
    local: UnixStream,
    target: &ProviderTarget,
    request: &Request,
    shutdown: &Arc<AtomicBool>,
) -> io::Result<()> {
    let config = curl_config(target, request)?;
    #[cfg(test)]
    let inject_monitor_error = MONITOR_ERROR_FD.load(Ordering::Acquire) == local.as_raw_fd();
    let monitor = local.try_clone()?;
    #[cfg(test)]
    if inject_monitor_error {
        MONITOR_ERROR_FD.store(monitor.as_raw_fd(), Ordering::Release);
    }
    let mut child = Command::new(crate::support::command::CURL);
    child
        .args(["-q", "--config", "-"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let child = child.spawn()?;
    #[cfg(test)]
    if MONITOR_ERROR_FD.load(Ordering::Acquire) == monitor.as_raw_fd() {
        MONITOR_ERROR_PID.store(child.id(), Ordering::Release);
    }
    let mut process = CurlProcess::new(child);
    let disconnected = Arc::new(AtomicBool::new(false));
    let run = run_curl_child(
        &mut process,
        local,
        &config,
        &monitor,
        shutdown,
        &disconnected,
    );
    let terminate = !matches!(run, Ok(CurlStop::Exited));
    let finished = process.finish(terminate);
    match (run, finished) {
        (Err(error), _) | (_, Err(error)) => Err(error),
        (
            Ok(_stop),
            Ok(CurlFinish {
                copied: Err(error), ..
            }),
        ) => Err(error),
        (Ok(stop), Ok(finished))
            if finished.status.success()
                || stop == CurlStop::Cancelled
                || shutdown.load(Ordering::Acquire)
                || disconnected.load(Ordering::Acquire) =>
        {
            Ok(())
        }
        (Ok(_stop), Ok(_finished)) => Err(io::Error::other("provider curl failed")),
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CurlStop {
    Exited,
    Cancelled,
}

struct CurlProcess {
    child: Option<Child>,
    output: Option<JoinHandle<io::Result<u64>>>,
    errors: Option<JoinHandle<Vec<u8>>>,
}

struct CurlFinish {
    status: ExitStatus,
    copied: io::Result<u64>,
}

impl CurlProcess {
    fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            output: None,
            errors: None,
        }
    }

    fn child_mut(&mut self) -> io::Result<&mut Child> {
        self.child
            .as_mut()
            .ok_or_else(|| io::Error::other("curl child is unavailable"))
    }

    fn finish(&mut self, terminate: bool) -> io::Result<CurlFinish> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| io::Error::other("curl child is unavailable"))?;
        if terminate {
            terminate_process_group(&mut child);
        }
        let status = child.wait();
        let copied = join_output(self.output.take());
        let errors = join_errors(self.errors.take());
        errors?;
        Ok(CurlFinish {
            status: status?,
            copied,
        })
    }
}

impl Drop for CurlProcess {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _finished = self.finish(true);
        }
    }
}

fn run_curl_child(
    process: &mut CurlProcess,
    local: UnixStream,
    config: &str,
    monitor: &UnixStream,
    shutdown: &AtomicBool,
    disconnected: &Arc<AtomicBool>,
) -> io::Result<CurlStop> {
    let mut stdin = process
        .child_mut()?
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("missing curl stdin"))?;
    stdin.write_all(config.as_bytes())?;
    drop(stdin);
    let mut stdout = process
        .child_mut()?
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing curl stdout"))?;
    let stderr = process
        .child_mut()?
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing curl stderr"))?;
    let output_disconnected = Arc::clone(disconnected);
    process.output = Some(
        thread::Builder::new()
            .name("egress-curl-output".to_owned())
            .spawn(move || {
                let mut local = local;
                let result = io::copy(&mut stdout, &mut local);
                if result.is_err() {
                    output_disconnected.store(true, Ordering::Release);
                }
                result
            })?,
    );
    process.errors = Some(
        thread::Builder::new()
            .name("egress-curl-errors".to_owned())
            .spawn(move || read_limited_bytes(stderr, STDERR_MAX))?,
    );
    loop {
        if shutdown.load(Ordering::Acquire)
            || disconnected.load(Ordering::Acquire)
            || client_closed(monitor)?
        {
            return Ok(CurlStop::Cancelled);
        }
        if process.child_mut()?.try_wait()?.is_some() {
            return Ok(CurlStop::Exited);
        }
        thread::sleep(IO_PAUSE);
    }
}

fn join_output(output: Option<JoinHandle<io::Result<u64>>>) -> io::Result<u64> {
    output.map_or(Ok(0), |output| {
        output
            .join()
            .map_err(|_panic| io::Error::other("curl output thread panicked"))?
    })
}

fn join_errors(errors: Option<JoinHandle<Vec<u8>>>) -> io::Result<()> {
    errors.map_or(Ok(()), |errors| {
        errors
            .join()
            .map(|_stderr| ())
            .map_err(|_panic| io::Error::other("curl stderr thread panicked"))
    })
}

fn curl_config(target: &ProviderTarget, request: &Request) -> io::Result<String> {
    let url = format!("{}/{}", target.base_url, request.endpoint);
    let mut config = String::from(
        "request = \"POST\"\nhttp1.1\ninclude\nraw\nno-buffer\nno-location\nconnect-timeout = 5\nmax-time = 300\nsilent\nshow-error\n",
    );
    config.push_str("url = ");
    config.push_str(&curl_quote(url.as_bytes())?);
    config.push('\n');
    for header in &request.headers {
        config.push_str("header = ");
        config.push_str(&curl_quote(
            format!("{}: {}", header.0, header.1).as_bytes(),
        )?);
        config.push('\n');
    }
    config.push_str("data-binary = ");
    config.push_str(&curl_quote(&request.body)?);
    config.push('\n');
    Ok(config)
}

fn curl_quote(value: &[u8]) -> io::Result<String> {
    let value =
        std::str::from_utf8(value).map_err(|_error| invalid("provider HTTP value is not UTF-8"))?;
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' | '\"' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if !character.is_control() => quoted.push(character),
            _ => return Err(invalid("provider HTTP value is not curl-config safe")),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

fn client_closed(stream: &UnixStream) -> io::Result<bool> {
    #[cfg(test)]
    if MONITOR_ERROR_FD.load(Ordering::Acquire) == stream.as_raw_fd()
        && MONITOR_ERROR_ARMED.swap(false, Ordering::AcqRel)
    {
        return Err(io::Error::from_raw_os_error(nix::libc::EBADF));
    }
    let mut byte = [0_u8; 1];
    match nix::sys::socket::recv(
        stream.as_raw_fd(),
        &mut byte,
        nix::sys::socket::MsgFlags::MSG_PEEK | nix::sys::socket::MsgFlags::MSG_DONTWAIT,
    ) {
        Ok(0) => Ok(true),
        Ok(_) | Err(nix::errno::Errno::EAGAIN) => Ok(false),
        Err(error) => Err(io::Error::from(error)),
    }
}

#[cfg(test)]
static MONITOR_ERROR_ARMED: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static MONITOR_ERROR_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
#[cfg(test)]
static MONITOR_ERROR_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
