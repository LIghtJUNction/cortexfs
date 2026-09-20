use std::collections::BTreeSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::Server;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_TOOLS: usize = 1024;
const MAX_PAGES: usize = 64;
const MAX_FRAMES: usize = 1024;
const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
const LEGACY_VERSIONS: [&str; 4] = [
    LEGACY_PROTOCOL_VERSION,
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];
const DROP_GRACE: Duration = Duration::from_millis(100);
const TERM_WAIT: Duration = Duration::from_millis(400);
const KILL_WAIT: Duration = Duration::from_millis(400);
const READER_WAIT: Duration = Duration::from_millis(200);
const DISCOVER_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_millis(40)
} else {
    Duration::from_secs(5)
};
const TIMEOUT: Duration = if cfg!(test) {
    Duration::from_millis(250)
} else {
    Duration::from_secs(30)
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TaskSupport {
    #[default]
    Forbidden,
    Optional,
    Required,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ToolExecution {
    #[serde(default, rename = "taskSupport")]
    pub(crate) task_support: TaskSupport,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RemoteTool {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(rename = "inputSchema")]
    pub(crate) schema: Value,
    #[serde(default)]
    pub(crate) execution: ToolExecution,
}

pub(crate) struct Client {
    child: Child,
    input: Option<ChildStdin>,
    events: Option<Receiver<io::Result<Vec<u8>>>>,
    reader: Option<JoinHandle<io::Result<()>>>,
    stderr_reader: Option<JoinHandle<io::Result<()>>>,
    deadline: Instant,
    protocol: Option<&'static str>,
    next_id: u64,
    total: usize,
    frames: usize,
    finished: bool,
}

impl Client {
    pub(crate) fn start(server: &Server) -> io::Result<Self> {
        if server.command.is_empty()
            || server.command.chars().any(char::is_control)
            || server
                .args
                .iter()
                .any(|arg| arg.chars().any(|character| character == '\0'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid MCP command",
            ));
        }
        let mut command = Command::new("/usr/bin/setsid");
        command
            .arg("--")
            .arg(&server.command)
            .args(&server.args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .envs(&server.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("MCP stdin unavailable"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("MCP stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("MCP stderr unavailable"))?;
        let (sender, events) = mpsc::sync_channel(1);
        let stderr_reader = std::thread::spawn(move || read_stderr(stderr));
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let result = reader
                    .by_ref()
                    .take(
                        u64::try_from(MAX_BYTES)
                            .unwrap_or(u64::MAX)
                            .saturating_add(1),
                    )
                    .read_until(b'\n', &mut bytes)
                    .and_then(|read| {
                        if read == 0 {
                            return Err(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "MCP server closed stdout",
                            ));
                        }
                        if bytes.len() > MAX_BYTES || bytes.last() != Some(&b'\n') {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "MCP frame exceeds 1 MiB",
                            ));
                        }
                        Ok(bytes)
                    });
                let stop = result.is_err();
                if sender.send(result).is_err() || stop {
                    return Ok(());
                }
            }
        });
        let mut client = Self {
            child,
            input: Some(input),
            events: Some(events),
            reader: Some(reader),
            stderr_reader: Some(stderr_reader),
            deadline: Instant::now() + TIMEOUT,
            protocol: None,
            next_id: 1,
            total: 0,
            frames: 0,
            finished: false,
        };
        if !client.discover()? {
            client.initialize()?;
        }
        Ok(client)
    }

    fn discover(&mut self) -> io::Result<bool> {
        let deadline = self.deadline;
        self.deadline = deadline.min(Instant::now() + DISCOVER_TIMEOUT);
        let response = self.exchange(
            "server/discover",
            &modern_params(Map::new(), MODERN_PROTOCOL_VERSION),
        );
        self.deadline = deadline;
        let response = match response {
            Err(error) if error.kind() == io::ErrorKind::TimedOut => return Ok(false),
            result => result?,
        };
        if let Some(error) = response.get("error") {
            match error["code"].as_i64() {
                Some(-32020 | -32021) => return Err(invalid_data("modern MCP discovery error")),
                Some(-32022) => {}
                _ => return Ok(false),
            }
            let data = error
                .get("data")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid_data("invalid unsupported-version error"))?;
            let supported = data
                .get("supported")
                .and_then(Value::as_array)
                .filter(|values| values.iter().all(Value::is_string))
                .ok_or_else(|| invalid_data("invalid unsupported-version error"))?;
            if data.get("requested").and_then(Value::as_str) != Some(MODERN_PROTOCOL_VERSION)
                || supported
                    .iter()
                    .any(|version| version.as_str() == Some(MODERN_PROTOCOL_VERSION))
            {
                return Err(invalid_data("invalid unsupported-version error"));
            }
            return Err(invalid_data("no mutually supported modern MCP version"));
        }
        let result = response
            .get("result")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid_data("invalid server/discover result"))?;
        let supported = result
            .get("supportedVersions")
            .and_then(Value::as_array)
            .filter(|values| values.iter().all(Value::is_string))
            .ok_or_else(|| invalid_data("invalid server/discover result"))?;
        let tools = result
            .get("capabilities")
            .and_then(Value::as_object)
            .and_then(|capabilities| capabilities.get("tools"))
            .is_some_and(Value::is_object);
        if result
            .get("resultType")
            .is_some_and(|value| value.as_str() != Some("complete"))
            || !tools
            || !supported
                .iter()
                .any(|version| version.as_str() == Some(MODERN_PROTOCOL_VERSION))
        {
            return Err(invalid_data("incompatible server/discover result"));
        }
        self.protocol = Some(MODERN_PROTOCOL_VERSION);
        Ok(true)
    }

    fn initialize(&mut self) -> io::Result<()> {
        let initialized = self.request(
            "initialize",
            &json!({
                "protocolVersion":LEGACY_PROTOCOL_VERSION,
                "capabilities":{},
                "clientInfo":{"name":"ctxmcp","version":env!("CARGO_PKG_VERSION")}
            }),
        )?;
        let valid_version = initialized
            .get("protocolVersion")
            .and_then(Value::as_str)
            .is_some_and(|version| LEGACY_VERSIONS.contains(&version));
        let tools = initialized
            .get("capabilities")
            .and_then(Value::as_object)
            .and_then(|capabilities| capabilities.get("tools"))
            .is_some_and(Value::is_object);
        if !valid_version || !tools {
            return Err(invalid_data("invalid initialize result"));
        }
        self.notify("notifications/initialized", &json!({}))
    }

    pub(crate) fn tools(&mut self) -> io::Result<Vec<RemoteTool>> {
        let result = (|| {
            let mut cursor: Option<String> = None;
            let mut tools = Vec::new();
            let mut names = BTreeSet::new();
            for _page in 0..MAX_PAGES {
                let params = cursor
                    .as_ref()
                    .map_or_else(|| json!({}), |cursor| json!({"cursor":cursor}));
                let result = self.request("tools/list", &params)?;
                let page = result
                    .get("tools")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid_data("invalid tools/list result"))?;
                for value in page {
                    let tool: RemoteTool = serde_json::from_value(value.clone())
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    if tool.schema.get("type").and_then(Value::as_str) != Some("object")
                        || !names.insert(tool.name.clone())
                        || tools.len() >= MAX_TOOLS
                    {
                        return Err(invalid_data("invalid, duplicate, or excessive MCP tool"));
                    }
                    tools.push(tool);
                }
                cursor = match result.get("nextCursor") {
                    None => None,
                    Some(value) if value.is_null() => None,
                    Some(value) => Some(
                        value
                            .as_str()
                            .ok_or_else(|| invalid_data("invalid tools/list cursor"))?
                            .to_owned(),
                    ),
                };
                if cursor.is_none() {
                    return Ok(tools);
                }
            }
            Err(invalid_data("MCP pagination exceeds 64 pages"))
        })();
        self.finish(result)
    }

    pub(crate) fn call(&mut self, name: &str, arguments: &Value) -> io::Result<Value> {
        let result = self.request("tools/call", &json!({"name":name,"arguments":arguments}));
        self.finish(result)
    }

    fn request(&mut self, method: &str, params: &Value) -> io::Result<Value> {
        let params = if let Some(version) = self.protocol {
            modern_params(
                params
                    .as_object()
                    .cloned()
                    .ok_or_else(|| invalid_data("MCP params must be an object"))?,
                version,
            )
        } else {
            params.clone()
        };
        let response = self.exchange(method, &params)?;
        if let Some(error) = response.get("error") {
            return Err(io::Error::other(format!(
                "MCP request failed: {}",
                error["code"]
            )));
        }
        response
            .get("result")
            .cloned()
            .filter(|result| {
                self.protocol.is_none()
                    || result
                        .get("resultType")
                        .is_none_or(|value| value.as_str() == Some("complete"))
            })
            .ok_or_else(|| invalid_data("missing or unsupported JSON-RPC result"))
    }

    fn exchange(&mut self, method: &str, params: &Value) -> io::Result<Value> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.write(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        loop {
            let value = self.read()?;
            if value.get("method").is_some() {
                if value.get("id").is_some() {
                    if self.protocol.is_some() || method == "server/discover" {
                        return Err(invalid_data("unsupported modern MCP server request"));
                    }
                    self.reply_server_request(&value)?;
                } else if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
                    || value.get("method").and_then(Value::as_str).is_none()
                    || !matches!(
                        value.get("params"),
                        None | Some(Value::Object(_) | Value::Array(_))
                    )
                {
                    return Err(invalid_data("invalid JSON-RPC notification"));
                }
                continue;
            }
            if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
                || value.get("result").is_some() == value.get("error").is_some()
                || value.get("error").is_some_and(|error| {
                    !(error["code"].is_i64() || error["code"].is_u64())
                        || !error["message"].is_string()
                })
            {
                return Err(invalid_data("invalid JSON-RPC response"));
            }
            let response_id = value.get("id").and_then(Value::as_u64);
            if response_id.is_some_and(|response_id| response_id < id) {
                continue;
            }
            if response_id != Some(id) {
                return Err(invalid_data("invalid JSON-RPC response"));
            }
            return Ok(value);
        }
    }

    fn reply_server_request(&mut self, value: &Value) -> io::Result<()> {
        let id = value.get("id").filter(|id| id.is_string() || id.is_number());
        let valid = value.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
            && value.get("method").and_then(Value::as_str) == Some("ping")
            && id.is_some()
            && value.get("params").is_none_or(Value::is_object)
            && value.get("result").is_none()
            && value.get("error").is_none();
        if !valid {
            return Err(invalid_data("unsupported or invalid MCP server request"));
        }
        self.write(&json!({"jsonrpc":"2.0","id":id,"result":{}}))
    }

    fn notify(&mut self, method: &str, params: &Value) -> io::Result<()> {
        self.write(&json!({"jsonrpc":"2.0","method":method,"params":params}))
    }

    fn write(&mut self, value: &Value) -> io::Result<()> {
        let bytes = serde_json::to_vec(value)?;
        let frame_len = bytes.len().saturating_add(1);
        if frame_len > MAX_BYTES || self.total.saturating_add(frame_len) > MAX_BYTES {
            return Err(invalid_data("MCP traffic exceeds 1 MiB"));
        }
        self.total = self.total.saturating_add(frame_len);
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "MCP stdin is closed"))?;
        input.write_all(&bytes)?;
        input.write_all(b"\n")?;
        input.flush()
    }

    fn read(&mut self) -> io::Result<Value> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "MCP response timeout",
            ));
        }
        let event = self
            .events
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "MCP reader stopped"))?
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    io::Error::new(io::ErrorKind::TimedOut, "MCP response timeout")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "MCP reader stopped")
                }
            })?;
        let bytes = event?;
        self.frames = self.frames.saturating_add(1);
        if self.frames > MAX_FRAMES {
            return Err(invalid_data("MCP response exceeds frame limit"));
        }
        if self.total.saturating_add(bytes.len()) > MAX_BYTES {
            return Err(invalid_data("MCP traffic exceeds 1 MiB"));
        }
        self.total = self.total.saturating_add(bytes.len());
        serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn finish<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        self.shutdown()?;
        result
    }

    fn shutdown(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        drop(self.input.take());
        let process = stop_process_group(&mut self.child);
        self.finished = process.is_ok();
        drop(self.events.take());
        let deadline = Instant::now() + READER_WAIT;
        let output = finish_thread(self.reader.take(), deadline);
        let errors = finish_thread(self.stderr_reader.take(), deadline);
        errors?;
        output?;
        process
    }
}

fn modern_params(mut params: Map<String, Value>, version: &str) -> Value {
    params.insert(
        "_meta".to_owned(),
        json!({
            "io.modelcontextprotocol/protocolVersion":version,
            "io.modelcontextprotocol/clientInfo":{
                "name":"ctxmcp",
                "version":env!("CARGO_PKG_VERSION")
            },
            "io.modelcontextprotocol/clientCapabilities":{}
        }),
    );
    Value::Object(params)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn read_stderr(stderr: std::process::ChildStderr) -> io::Result<()> {
    let limit = u64::try_from(MAX_BYTES).unwrap_or(u64::MAX);
    let read = io::copy(&mut stderr.take(limit.saturating_add(1)), &mut io::sink())?;
    if read > limit {
        return Err(invalid_data("MCP stderr exceeds 1 MiB"));
    }
    Ok(())
}

impl Drop for Client {
    fn drop(&mut self) {
        let _shutdown = self.shutdown();
    }
}

fn stop_process_group(child: &mut Child) -> io::Result<()> {
    let process_group = i32::try_from(child.id()).ok().map(nix::unistd::Pid::from_raw);
    let _graceful = wait_child(child, DROP_GRACE)?;
    if let Some(process_group) = process_group {
        signal_process_group(process_group, nix::sys::signal::Signal::SIGTERM)?;
        if !wait_process_group(child, process_group, TERM_WAIT)? {
            signal_process_group(process_group, nix::sys::signal::Signal::SIGKILL)?;
            if !wait_process_group(child, process_group, KILL_WAIT)? {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "MCP process group shutdown timeout",
                ));
            }
        }
    }
    if !wait_child(child, DROP_GRACE)? {
        child.kill()?;
        if !wait_child(child, DROP_GRACE)? {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "MCP child shutdown timeout",
            ));
        }
    }
    Ok(())
}

fn signal_process_group(
    process_group: nix::unistd::Pid,
    signal: nix::sys::signal::Signal,
) -> io::Result<()> {
    match nix::sys::signal::killpg(process_group, signal) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
        Err(error) => Err(io::Error::from(error)),
    }
}

fn wait_process_group(
    child: &mut Child,
    process_group: nix::unistd::Pid,
    timeout: Duration,
) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        let _reaped = child.try_wait()?;
        match nix::sys::signal::killpg(process_group, None) {
            Err(nix::errno::Errno::ESRCH) => return Ok(true),
            Ok(()) => {}
            Err(error) => return Err(io::Error::from(error)),
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn finish_thread(handle: Option<JoinHandle<io::Result<()>>>, deadline: Instant) -> io::Result<()> {
    let Some(handle) = handle else {
        return Ok(());
    };
    while !handle.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    if !handle.is_finished() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "MCP reader shutdown timeout",
        ));
    }
    handle
        .join()
        .map_err(|_panic| io::Error::other("MCP reader panicked"))?
}

fn wait_child(child: &mut Child, timeout: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests;
