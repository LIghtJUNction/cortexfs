use std::collections::BTreeSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::Server;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_TOOLS: usize = 1024;
const MAX_PAGES: usize = 64;
const MAX_FRAMES: usize = 1024;
const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_VERSIONS: [&str; 4] = [
    LATEST_PROTOCOL_VERSION,
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];
const DROP_GRACE: Duration = Duration::from_millis(100);
const TERM_WAIT: Duration = Duration::from_millis(400);
const KILL_WAIT: Duration = Duration::from_millis(400);
const READER_WAIT: Duration = Duration::from_millis(200);
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
            next_id: 1,
            total: 0,
            frames: 0,
            finished: false,
        };
        let initialized = client.request(
            "initialize",
            &json!({
                "protocolVersion":LATEST_PROTOCOL_VERSION,
                "capabilities":{},
                "clientInfo":{"name":"ctxmcp","version":env!("CARGO_PKG_VERSION")}
            }),
        )?;
        let valid_version = initialized
            .get("protocolVersion")
            .and_then(Value::as_str)
            .is_some_and(|version| SUPPORTED_VERSIONS.contains(&version));
        let tools_capability = initialized
            .get("capabilities")
            .and_then(Value::as_object)
            .and_then(|capabilities| capabilities.get("tools"))
            .is_some_and(Value::is_object);
        if !valid_version || !tools_capability {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid initialize result",
            ));
        }
        client.notify("notifications/initialized", &json!({}))?;
        Ok(client)
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
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid tools/list result")
                    })?;
                for value in page {
                    let tool: RemoteTool = serde_json::from_value(value.clone())
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    if tool.schema.get("type").and_then(Value::as_str) != Some("object")
                        || !names.insert(tool.name.clone())
                        || tools.len() >= MAX_TOOLS
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid, duplicate, or excessive MCP tool",
                        ));
                    }
                    tools.push(tool);
                }
                cursor = match result.get("nextCursor") {
                    None => None,
                    Some(value) if value.is_null() => None,
                    Some(value) => Some(
                        value
                            .as_str()
                            .ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "invalid tools/list cursor",
                                )
                            })?
                            .to_owned(),
                    ),
                };
                if cursor.is_none() {
                    return Ok(tools);
                }
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP pagination exceeds 64 pages",
            ))
        })();
        self.finish(result)
    }

    pub(crate) fn call(&mut self, name: &str, arguments: &Value) -> io::Result<Value> {
        let result = self.request("tools/call", &json!({"name":name,"arguments":arguments}));
        self.finish(result)
    }

    fn request(&mut self, method: &str, params: &Value) -> io::Result<Value> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.write(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        let value = loop {
            let value = self.read()?;
            if value.get("method").is_some() {
                if value.get("id").is_some() {
                    self.reply_server_request(&value)?;
                    continue;
                }
                if value.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
                    && value.get("method").and_then(Value::as_str).is_some()
                {
                    continue;
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid JSON-RPC notification",
                ));
            }
            break value;
        };
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || value.get("id").and_then(Value::as_u64) != Some(id)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid JSON-RPC response",
            ));
        }
        if let Some(error) = value.get("error") {
            return Err(io::Error::other(format!(
                "MCP request failed: {}",
                error
                    .get("code")
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
            )));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing JSON-RPC result"))
    }

    fn reply_server_request(&mut self, value: &Value) -> io::Result<()> {
        let id = value
            .get("id")
            .filter(|id| id.is_string() || id.is_number());
        let valid = value.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
            && value.get("method").and_then(Value::as_str) == Some("ping")
            && id.is_some()
            && value.get("params").is_none_or(Value::is_object)
            && value.get("result").is_none()
            && value.get("error").is_none();
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported or invalid MCP server request",
            ));
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
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP traffic exceeds 1 MiB",
            ));
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
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP response exceeds frame limit",
            ));
        }
        if self.total.saturating_add(bytes.len()) > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP traffic exceeds 1 MiB",
            ));
        }
        self.total = self.total.saturating_add(bytes.len());
        serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// Closes the current reader stream and returns the wrapped result.
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

fn read_stderr(stderr: std::process::ChildStderr) -> io::Result<()> {
    let limit = u64::try_from(MAX_BYTES).unwrap_or(u64::MAX);
    let read = io::copy(&mut stderr.take(limit.saturating_add(1)), &mut io::sink())?;
    if read > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MCP stderr exceeds 1 MiB",
        ));
    }
    Ok(())
}

impl Drop for Client {
    fn drop(&mut self) {
        let _shutdown = self.shutdown();
    }
}

fn stop_process_group(child: &mut Child) -> io::Result<()> {
    let process_group = i32::try_from(child.id())
        .ok()
        .map(nix::unistd::Pid::from_raw);
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
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;

    fn server(mode: &str) -> Server {
        let script = r#"
import json, os, sys, time
mode=os.environ.get("CTXMCP_MOCK")
if os.environ.get("PID_FILE"):
 open(os.environ["PID_FILE"],"w").write(str(os.getpid()))
if mode=="timeout":
 time.sleep(2); sys.exit(0)
if mode=="oversize":
 sys.stdout.write("x"*1048577); sys.stdout.flush(); sys.exit(0)
page=0
for line in sys.stdin:
 r=json.loads(line)
 if "id" not in r:
  if mode=="descendant":
   child=os.fork()
   if child==0:
    with open(os.environ["DESC_PID_FILE"],"w") as f: f.write(str(os.getpid()))
    while True: time.sleep(1)
   sys.exit(0)
  continue
 m=r.get("method")
 if mode in ("stderrlimit","stderroverflow") and m=="initialize":
  size=1048576 if mode=="stderrlimit" else 1048577
  try:
   sys.stderr.buffer.write(b"x"*size); sys.stderr.buffer.flush()
  except BrokenPipeError:
   sys.stderr=open(os.devnull,"w")
 if mode=="slowframes" and m=="initialize":
  for _ in range(4):
   print(json.dumps({"jsonrpc":"2.0","method":"progress"}),flush=True); time.sleep(.1)
 if mode=="invalidnotification" and m=="initialize":
  print(json.dumps({"method":"progress"}),flush=True)
 if mode in ("pingstr","pingnum") and m=="initialize":
  ping_id="server-ping" if mode=="pingstr" else 7
  print(json.dumps({"jsonrpc":"2.0","id":ping_id,"method":"ping","params":{}}),flush=True)
  reply=json.loads(sys.stdin.readline())
  if reply!={"jsonrpc":"2.0","id":ping_id,"result":{}}: sys.exit(3)
 if mode=="unsupportedrequest" and m=="initialize":
  print(json.dumps({"jsonrpc":"2.0","id":"server-request","method":"roots/list","params":{}}),flush=True)
 if mode=="malformedping" and m=="initialize":
  print(json.dumps({"jsonrpc":"2.0","id":None,"method":"ping","params":{}}),flush=True)
 versions={"legacy20241105":"2024-11-05","stable20250326":"2025-03-26","stable20250618":"2025-06-18","unknownversion":"unknown","draftversion":"2025-11-25-draft","badversion":"2099-01-01"}
 version=versions.get(mode,"2025-11-25")
 if m=="initialize":
  capabilities={"tools":{}}
  if mode=="missingtools": capabilities={}
  if mode=="badtools": capabilities={"tools":[]}
  result={"protocolVersion":version,"capabilities":capabilities,"serverInfo":{"name":"mock","version":"1"}}
 elif m=="tools/list" and page==0:
  page=1
  tool={"name":"echo","title":"Echo title","description":"Echo","icons":[{"src":"data:image/svg+xml,echo","mimeType":"image/svg+xml","sizes":["any"]}],"annotations":{"readOnlyHint":True},"inputSchema":{"type":"object"},"outputSchema":{"type":"object"},"execution":{"taskSupport":"forbidden"},"_meta":{"fixture":"current"}}
  if mode=="missingname": tool.pop("name")
  if mode=="missinginputschema": tool.pop("inputSchema")
  if mode=="missingschematype": tool["inputSchema"]={}
  if mode=="wrongschematype": tool["inputSchema"]={"type":"string"}
  if mode=="optionaltask": tool["execution"]={"taskSupport":"optional"}
  if mode=="requiredtask": tool["execution"]={"taskSupport":"required"}
  if mode=="badtask": tool["execution"]={"taskSupport":"unknown"}
  result={"tools":[tool],"nextCursor":7 if mode=="badcursor" else "next"}
 elif m=="tools/list":
  name="echo" if mode=="duplicate" else "sum"
  result={"tools":[{"name":name,"description":"Sum","inputSchema":{"type":"object"}}]}
 elif m=="tools/call": result={"content":[{"type":"text","text":"ok"}],"isError":False}
 else: result={}
 if mode=="error" and m=="tools/list":
  print(json.dumps({"jsonrpc":"2.0","id":r["id"],"error":{"code":-32603,"message":"secret-value"}}),flush=True); continue
 if mode=="callerror" and m=="tools/call":
  print(json.dumps({"jsonrpc":"2.0","id":r["id"],"error":{"code":-32603,"message":"failed"}}),flush=True); continue
 print(json.dumps({"jsonrpc":"2.0","id":r["id"],"result":result}),flush=True)
"#;
        Server {
            command: "/usr/bin/python3".to_owned(),
            args: vec!["-u".to_owned(), "-c".to_owned(), script.to_owned()],
            env: BTreeMap::from([("CTXMCP_MOCK".to_owned(), mode.to_owned())]),
        }
    }

    fn server_with_pid(mode: &str, path: &std::path::Path) -> Server {
        let mut value = server(mode);
        value
            .env
            .insert("PID_FILE".to_owned(), path.to_string_lossy().into_owned());
        value
    }

    fn server_with_descendant(parent: &std::path::Path, descendant: &std::path::Path) -> Server {
        let mut value = server("descendant");
        value
            .env
            .insert("PID_FILE".to_owned(), parent.to_string_lossy().into_owned());
        value.env.insert(
            "DESC_PID_FILE".to_owned(),
            descendant.to_string_lossy().into_owned(),
        );
        value
    }

    fn wait_pid(path: &std::path::Path, timeout: Duration) -> io::Result<nix::unistd::Pid> {
        let deadline = Instant::now() + timeout;
        loop {
            match fs::read_to_string(path) {
                Ok(value) => {
                    return value
                        .parse::<i32>()
                        .map(nix::unistd::Pid::from_raw)
                        .map_err(io::Error::other);
                }
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn assert_reaped(path: &std::path::Path) -> io::Result<()> {
        let pid = fs::read_to_string(path)?
            .parse::<i32>()
            .map_err(io::Error::other)?;
        let pid = nix::unistd::Pid::from_raw(pid);
        assert_eq!(
            nix::sys::signal::kill(pid, None),
            Err(nix::errno::Errno::ESRCH)
        );
        assert_eq!(
            nix::sys::signal::killpg(pid, None),
            Err(nix::errno::Errno::ESRCH)
        );
        Ok(())
    }

    #[test]
    fn initialize_lists_pages_and_calls() -> io::Result<()> {
        let mut client = Client::start(&server("ok"))?;
        let tools = client.tools()?;
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            ["echo", "sum"]
        );
        let mut client = Client::start(&server("ok"))?;
        assert_eq!(
            client.call("echo", &json!({"text":"hi"}))?.get("isError"),
            Some(&Value::Bool(false))
        );
        Ok(())
    }

    #[test]
    fn server_ping_requests_with_string_and_number_ids_are_answered() -> io::Result<()> {
        for mode in ["pingstr", "pingnum"] {
            let mut client = Client::start(&server(mode))?;
            drop(client.tools()?);
        }
        Ok(())
    }

    #[test]
    fn unsupported_and_malformed_server_requests_are_rejected() {
        for mode in ["unsupportedrequest", "malformedping"] {
            assert!(
                Client::start(&server(mode)).is_err(),
                "mode {mode} accepted"
            );
        }
    }

    #[test]
    fn stable_protocol_versions_are_accepted() -> io::Result<()> {
        for mode in ["ok", "stable20250618", "stable20250326", "legacy20241105"] {
            let mut client = Client::start(&server(mode))?;
            drop(client.tools()?);
        }
        Ok(())
    }

    #[test]
    fn unknown_draft_and_future_protocol_versions_are_rejected() {
        for mode in ["unknownversion", "draftversion", "badversion"] {
            assert!(
                Client::start(&server(mode)).is_err(),
                "mode {mode} accepted"
            );
        }
    }

    #[test]
    fn missing_or_malformed_tools_capability_is_rejected() {
        for mode in ["missingtools", "badtools"] {
            assert!(
                Client::start(&server(mode)).is_err(),
                "mode {mode} accepted"
            );
        }
    }

    #[test]
    fn required_tool_shape_and_object_schema_type_are_enforced() -> io::Result<()> {
        for mode in [
            "missingname",
            "missinginputschema",
            "missingschematype",
            "wrongschematype",
        ] {
            let mut client = Client::start(&server(mode))?;
            assert!(client.tools().is_err(), "mode {mode} accepted");
        }
        Ok(())
    }

    #[test]
    fn task_support_defaults_and_stable_values_are_decoded() -> io::Result<()> {
        let mut default = Client::start(&server("ok"))?;
        assert!(
            default
                .tools()?
                .iter()
                .all(|tool| tool.execution.task_support == TaskSupport::Forbidden)
        );
        let mut optional = Client::start(&server("optionaltask"))?;
        assert_eq!(
            optional
                .tools()?
                .first()
                .map(|tool| tool.execution.task_support),
            Some(TaskSupport::Optional)
        );
        let mut required = Client::start(&server("requiredtask"))?;
        assert_eq!(
            required
                .tools()?
                .first()
                .map(|tool| tool.execution.task_support),
            Some(TaskSupport::Required)
        );
        let mut bad = Client::start(&server("badtask"))?;
        assert!(bad.tools().is_err());
        Ok(())
    }

    #[test]
    fn timeout_and_oversize_are_rejected() {
        assert_eq!(
            Client::start(&server("timeout"))
                .err()
                .map(|error| error.kind()),
            Some(io::ErrorKind::TimedOut)
        );
        assert_eq!(
            Client::start(&server("oversize"))
                .err()
                .map(|error| error.kind()),
            Some(io::ErrorKind::InvalidData)
        );
    }

    #[test]
    fn stderr_has_an_independent_hard_limit() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let limit_pid = root.path().join("limit-pid");
        let mut limit = Client::start(&server_with_pid("stderrlimit", &limit_pid))?;
        drop(limit.tools()?);
        assert_reaped(&limit_pid)?;

        let overflow_pid = root.path().join("overflow-pid");
        assert_eq!(
            Client::start(&server_with_pid("stderroverflow", &overflow_pid))
                .and_then(|mut client| client.tools())
                .err()
                .map(|error| error.kind()),
            Some(io::ErrorKind::InvalidData)
        );
        assert_reaped(&overflow_pid)
    }

    #[test]
    fn rpc_errors_and_duplicate_names_are_rejected() -> io::Result<()> {
        let mut error = Client::start(&server("error"))?;
        assert!(error.tools().is_err());
        let mut duplicate = Client::start(&server("duplicate"))?;
        assert!(duplicate.tools().is_err());
        Ok(())
    }

    #[test]
    fn cleanup_is_bounded() -> io::Result<()> {
        let client = Client::start(&server("ok"))?;
        let started = Instant::now();
        drop(client);
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    #[test]
    fn descendant_pipe_holder_is_killed_without_blocking_drop() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let parent = root.path().join("parent-pid");
        let descendant = root.path().join("descendant-pid");
        let client = Client::start(&server_with_descendant(&parent, &descendant))?;
        let descendant_pid = wait_pid(&descendant, Duration::from_secs(1))?;
        let started = Instant::now();
        drop(client);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_reaped(&parent)?;
        assert_eq!(
            nix::sys::signal::kill(descendant_pid, None),
            Err(nix::errno::Errno::ESRCH)
        );
        Ok(())
    }

    #[test]
    fn cursor_notifications_and_call_errors_are_strict() -> io::Result<()> {
        for mode in ["invalidnotification", "slowframes"] {
            assert!(
                Client::start(&server(mode)).is_err(),
                "mode {mode} accepted"
            );
        }
        let mut cursor = Client::start(&server("badcursor"))?;
        assert!(cursor.tools().is_err());
        let mut call = Client::start(&server("callerror"))?;
        assert!(call.call("echo", &json!({})).is_err());
        Ok(())
    }

    #[test]
    fn every_error_path_reaps_the_process_group() -> io::Result<()> {
        for mode in ["badversion", "timeout", "oversize"] {
            let root = tempfile::tempdir()?;
            let pid = root.path().join("pid");
            assert!(Client::start(&server_with_pid(mode, &pid)).is_err());
            assert_reaped(&pid)?;
        }
        let root = tempfile::tempdir()?;
        let pid = root.path().join("pid");
        let mut client = Client::start(&server_with_pid("callerror", &pid))?;
        assert!(client.call("echo", &json!({})).is_err());
        assert_reaped(&pid)
    }

    #[test]
    fn successful_call_reaps_before_return() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let pid = root.path().join("pid");
        let mut client = Client::start(&server_with_pid("ok", &pid))?;
        client.call("echo", &json!({}))?;
        assert_reaped(&pid)
    }
}
