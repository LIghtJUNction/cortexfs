fn list_objects(root: &Path, target: &LsTarget) -> Result<(), CliError> {
    for entry in list_names(root, target)? {
        print_line(&entry)?;
    }
    Ok(())
}

fn list_names(root: &Path, target: &LsTarget) -> Result<Vec<String>, CliError> {
    let LsPath { path, object_class } = resolve_ls_path(root, target)?;

    if let Some(kind) = object_class {
        return list_kind_names(root, kind);
    }

    read_dir_names(&path)
}

fn list_kind_names(root: &Path, kind: ObjectClass) -> Result<Vec<String>, CliError> {
    Ok(read_dir_names(&root.join(kind.as_str()))?
        .into_iter()
        .filter(|name| is_object_name(name))
        .collect())
}

struct LsPath {
    path: PathBuf,
    object_class: Option<ObjectClass>,
}

fn resolve_ls_path(root: &Path, target: &LsTarget) -> Result<LsPath, CliError> {
    let path = match *target {
        LsTarget::Root => return Ok(root_ls_path(root)),
        LsTarget::Path(ref path) => normalized_ls_path(path),
    };

    if path.is_empty() {
        return Ok(root_ls_path(root));
    }

    let resolved = resolve_abi_path(root, &path)?;
    let abi_path = classify_input_path(root, &path)?;
    let object_class = match abi_path.as_str() {
        "model" => Some(ObjectClass::Model),
        "agent" => Some(ObjectClass::Agent),
        "tool" => Some(ObjectClass::Tool),
        _ => None,
    };

    Ok(LsPath {
        path: resolved,
        object_class,
    })
}

fn root_ls_path(root: &Path) -> LsPath {
    LsPath {
        path: root.to_path_buf(),
        object_class: None,
    }
}

fn normalized_ls_path(path: &str) -> String {
    if path == "/" {
        return String::new();
    }

    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }

    trimmed.to_owned()
}

fn read_dir_names(dir: &Path) -> Result<Vec<String>, CliError> {
    let directory = open_read_dir_plain_directory(dir).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", dir.display()))
    })?;
    let fd_path = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    let entries = fs::read_dir(fd_path).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", dir.display()))
    })?;
    let mut names = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| {
            CliError::unavailable(format!("cannot read {} entry: {error}", dir.display()))
        })?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }

    names.sort();
    Ok(names)
}

fn open_read_dir_plain_directory(path: &Path) -> io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_single_read_dir_plain_directory(Path::new("/"))?
    } else {
        open_single_read_dir_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "directory path is not utf-8")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map_err(io::Error::from)?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_single_read_dir_plain_directory(path: &Path) -> io::Result<fs::File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a directory",
        ));
    }
    Ok(directory)
}

fn which_object(root: &Path, class: ObjectClass, name: &str) -> Result<(), CliError> {
    match class {
        ObjectClass::Model if !is_model_name(name) => {
            return Err(CliError::usage(format!("invalid model name: {name}")));
        }
        ObjectClass::Agent => require_cli_name("object name", name)?,
        ObjectClass::Tool => return which_tool(root, name),
        ObjectClass::Model => {}
    }

    let candidate = root.join(class.as_str()).join(name);
    if is_executable_file(&candidate) {
        return print_line(&candidate.display().to_string());
    }

    Err(CliError::unavailable(format!(
        "{} not found: {name}",
        class.as_str()
    )))
}

fn which_tool(root: &Path, name: &str) -> Result<(), CliError> {
    require_cli_name("object name", name)?;

    if let Some(hit) = ctx_tool_path(root)?.find(name).map_err(tool_path_error)? {
        return print_line(&hit.path().display().to_string());
    }

    Err(CliError::unavailable(format!("tool not found: {name}")))
}

fn run_visible_tool(root: &Path, name: &str, args: &[String]) -> Result<ExitCode, CliError> {
    run_visible_tool_with_writer(root, name, args, &mut io::stdout())
}

fn is_safe_direct_core_tool_cli(name: &str) -> bool {
    matches!(name, "tsh.config")
}

fn run_visible_tool_with_writer(
    root: &Path,
    name: &str,
    args: &[String],
    writer: &mut dyn Write,
) -> Result<ExitCode, CliError> {
    require_cli_name("tool name", name)?;
    let Some(_hit) = ctx_tool_path(root)?.find(name).map_err(tool_path_error)? else {
        return Err(CliError::unavailable(format!(
            "tool not found in CTX_PATH: {name}"
        )));
    };
    let cli_args = args.iter().map(OsString::from).collect::<Vec<_>>();
    if is_safe_direct_core_tool_cli(name)
        && let Some(code) = run_core_tool_cli_with_root(root, name, &cli_args, writer)
            .map_err(|error| CliError::unavailable(format!("tool {name} failed: {error}")))?
    {
        return Ok(code);
    }

    Err(CliError::unavailable(format!(
        "ctx tool {name} is disabled because direct CTX_PATH execution bypasses CortexFS tool authorization"
    )))
}

fn path_shared(root: &Path, name: &str) -> Result<(), CliError> {
    require_cli_name("shared name", name)?;
    print_line(&root.join("shared").join(name).display().to_string())
}

fn history(root: &Path, agent: &str, session: Option<&str>) -> Result<(), CliError> {
    let session_dir = agent_session_dir(root, agent, session)?;
    cat_path(&session_dir.join("messages.jsonl"))
}

fn latest(root: &Path, agent: &str, session: Option<&str>) -> Result<(), CliError> {
    let session_dir = agent_session_dir(root, agent, session)?;
    cat_path(&session_dir.join("latest.md"))
}

fn ping(root: &Path, path: &str) -> Result<ExitCode, CliError> {
    stream_socket_request(&object_socket_path(root, path)?, "{\"op\":\"ping\"}\n")
}

fn cancel(root: &Path, path: &str, run: &str) -> Result<ExitCode, CliError> {
    require_cli_name("run id", run)?;
    let request = format!("{{\"op\":\"cancel\",\"id\":{}}}\n", json_string(run));
    stream_socket_request(&object_socket_path(root, path)?, &request)
}

fn stream_socket_request(socket: &Path, request: &str) -> Result<ExitCode, CliError> {
    if request.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(CliError::usage(format!(
            "socket request exceeds {MAX_SOCKET_FRAME_BYTES} bytes: EMSGSIZE"
        )));
    }

    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CliError::unavailable(format!("cannot connect {}: {error}", socket.display()))
    })?;
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;

    let mut stdout = io::stdout().lock();
    io::copy(&mut stream, &mut stdout)
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))?;
    Ok(ExitCode::SUCCESS)
}

fn stream_socket_request_interruptible(
    socket: &Path,
    request: &str,
    interrupt: &AtomicBool,
) -> Result<bool, CliError> {
    if request.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(CliError::usage(format!(
            "socket request exceeds {MAX_SOCKET_FRAME_BYTES} bytes: EMSGSIZE"
        )));
    }

    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CliError::unavailable(format!("cannot connect {}: {error}", socket.display()))
    })?;
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| {
            CliError::unavailable(format!("cannot configure interruptible socket: {error}"))
        })?;
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;

    copy_socket_response_interruptible(stream, interrupt)
}

fn copy_socket_response_interruptible(
    mut stream: UnixStream,
    interrupt: &AtomicBool,
) -> Result<bool, CliError> {
    let mut stdout = io::stdout().lock();
    let mut buffer = [0_u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Ok(false),
            Ok(read) => {
                let Some(bytes) = buffer.get(..read) else {
                    return Err(CliError::unavailable("socket response read exceeded buffer"));
                };
                stdout
                    .write_all(bytes)
                    .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))?;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                if interrupt.load(Ordering::SeqCst) {
                    return Ok(true);
                }
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot read socket response: {error}"
                )));
            }
        }
    }
}

fn stream_agent_socket_request(
    socket: &Path,
    request: &str,
    raw: bool,
) -> Result<ExitCode, CliError> {
    if raw {
        return stream_socket_request(socket, request);
    }
    if request.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(CliError::usage(format!(
            "socket request exceeds {MAX_SOCKET_FRAME_BYTES} bytes: EMSGSIZE"
        )));
    }

    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CliError::unavailable(format!("cannot connect {}: {error}", socket.display()))
    })?;
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;

    render_agent_events(stream)
}

fn stream_agent_socket_request_streaming_interruptible(
    socket: &Path,
    request: &str,
    raw: bool,
    interrupt: Option<(&AgentInterruptGuard, &str, &str)>,
) -> Result<ExitCode, CliError> {
    if raw {
        if let Some((guard, cancel_request, run_id)) = interrupt {
            let interrupted =
                stream_socket_request_interruptible(socket, request, guard.interrupted_flag())?;
            if interrupted {
                write_terminal_error(&format!(
                    "ctx: interrupt requested; cancelling run {run_id}"
                ))?;
                return stream_socket_request(socket, cancel_request);
            }
            return Ok(ExitCode::SUCCESS);
        }
        return stream_socket_request(socket, request);
    }
    if request.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(CliError::usage(format!(
            "socket request exceeds {MAX_SOCKET_FRAME_BYTES} bytes: EMSGSIZE"
        )));
    }

    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CliError::unavailable(format!("cannot connect {}: {error}", socket.display()))
    })?;
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;

    let Some((guard, cancel_request, run_id)) = interrupt else {
        return render_agent_events(stream);
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| {
            CliError::unavailable(format!("cannot configure interruptible socket: {error}"))
        })?;
    let rendered = render_agent_events_interruptible(stream, guard.interrupted_flag())?;
    if rendered.interrupted {
        write_terminal_error(&format!("ctx: interrupt requested; cancelling run {run_id}"))?;
        send_socket_request_best_effort(socket, cancel_request)?;
    }
    Ok(ExitCode::from(rendered.exit_code))
}

#[cfg(test)]
fn stream_agent_socket_request_buffered_interruptible(
    socket: &Path,
    request: &str,
    raw: bool,
    interrupt: Option<(&AgentInterruptGuard, &str, &str)>,
) -> Result<ExitCode, CliError> {
    if raw {
        if let Some((guard, cancel_request, run_id)) = interrupt {
            let interrupted =
                stream_socket_request_interruptible(socket, request, guard.interrupted_flag())?;
            if interrupted {
                write_terminal_error(&format!(
                    "ctx: interrupt requested; cancelling run {run_id}"
                ))?;
                send_socket_request_best_effort(socket, cancel_request)?;
                return Ok(ExitCode::SUCCESS);
            }
            return Ok(ExitCode::SUCCESS);
        }
        return stream_socket_request(socket, request);
    }
    if request.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(CliError::usage(format!(
            "socket request exceeds {MAX_SOCKET_FRAME_BYTES} bytes: EMSGSIZE"
        )));
    }

    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CliError::unavailable(format!("cannot connect {}: {error}", socket.display()))
    })?;
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;

    if let Some((guard, cancel_request, run_id)) = interrupt {
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|error| {
                CliError::unavailable(format!("cannot configure interruptible socket: {error}"))
            })?;
        let rendered = render_agent_events_interruptible(stream, guard.interrupted_flag())?;
        if rendered.interrupted {
            write_terminal_error(&format!("ctx: interrupt requested; cancelling run {run_id}"))?;
            send_socket_request_best_effort(socket, cancel_request)?;
        }
        return Ok(ExitCode::SUCCESS);
    }

    render_agent_events(stream)
}

fn send_socket_request_best_effort(socket: &Path, request: &str) -> Result<(), CliError> {
    if request.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(CliError::usage(format!(
            "socket request exceeds {MAX_SOCKET_FRAME_BYTES} bytes: EMSGSIZE"
        )));
    }
    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CliError::unavailable(format!("cannot connect {}: {error}", socket.display()))
    })?;
    stream
        .set_write_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| {
            CliError::unavailable(format!("cannot configure cancel socket: {error}"))
        })?;
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct AgentEventRender {
    exit_code: u8,
    interrupted: bool,
}

fn render_agent_events(stream: UnixStream) -> Result<ExitCode, CliError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| {
            CliError::unavailable(format!("cannot configure socket progress: {error}"))
        })?;
    let reader = io::BufReader::new(stream);
    Ok(ExitCode::from(render_agent_event_lines(reader, None)?.exit_code))
}

fn render_agent_events_interruptible(
    stream: UnixStream,
    interrupt: &AtomicBool,
) -> Result<AgentEventRender, CliError> {
    let reader = io::BufReader::new(stream);
    render_agent_event_lines(reader, Some(interrupt))
}

fn render_agent_event_lines(
    mut reader: impl BufRead,
    interrupt: Option<&AtomicBool>,
) -> Result<AgentEventRender, CliError> {
    let mut saw_delta = false;
    let mut usage_totals = AgentUsageTotals::default();
    let mut exit = ExitCode::SUCCESS;
    let mut quiet_since = std::time::Instant::now();
    let mut next_waiting_notice = Duration::from_secs(3);
    let mut line = String::new();
    loop {
        line.clear();
        match read_agent_socket_event_line_limited(&mut reader, &mut line) {
            Ok(None) => break,
            Ok(Some(_bytes)) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                if interrupt.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                    return Ok(AgentEventRender {
                        exit_code: exit_code_u8(exit),
                        interrupted: true,
                    });
                }
                let elapsed = quiet_since.elapsed();
                if elapsed >= next_waiting_notice {
                    write_terminal_diagnostic(&waiting_diagnostic(elapsed.as_secs()))?;
                    next_waiting_notice += Duration::from_secs(3);
                }
                continue;
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot read socket response: {error}"
                )));
            }
        }
        quiet_since = std::time::Instant::now();
        next_waiting_notice = Duration::from_secs(3);
        let line = line.trim_end_matches(['\r', '\n']);
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            print_line(line)?;
            continue;
        };
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("delta" | "reasoning_delta") => {
                if let Some(text) = json_text_field(&value) {
                    print_terminal_text(text)?;
                    saw_delta = true;
                }
            }
            Some("message")
                if value.get("role").and_then(serde_json::Value::as_str) == Some("tool") =>
            {
                write_terminal_diagnostic(&tool_result_diagnostic(&value))?;
            }
            Some("message" | "reasoning_message") if !saw_delta => {
                if let Some(text) = json_text_field(&value) {
                    print_terminal_line(text)?;
                }
            }
            Some("tool_call") => {
                let name = value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("tool_call");
                write_terminal_diagnostic(&tool_running_diagnostic(name))?;
            }
            Some("usage") => {
                if let Some(diagnostic) = usage_totals.record_event(&value) {
                    write_terminal_diagnostic(&diagnostic)?;
                }
            }
            Some("debug") => {
                if let Some(diagnostic) = debug_timing_diagnostic(&value) {
                    write_terminal_diagnostic(&diagnostic)?;
                }
            }
            Some("error") => {
                let code = value
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("EIO");
                let message = value
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("runtime error");
                write_terminal_diagnostic(&error_diagnostic(code, message))?;
                exit = ExitCode::from(1);
            }
            Some("pong") => print_line("pong")?,
            Some("done") if saw_delta => {
                print_terminal_text("\n")?;
                saw_delta = false;
            }
            _ => {}
        }
    }
    Ok(AgentEventRender {
        exit_code: exit_code_u8(exit),
        interrupted: false,
    })
}

fn read_agent_socket_event_line_limited(
    reader: &mut impl BufRead,
    line: &mut String,
) -> io::Result<Option<usize>> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_SOCKET_FRAME_BYTES.saturating_add(1)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("socket frame read limit is invalid: {error}"),
        )
    })?;
    let read = reader.take(limit).read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "agent socket response frame exceeds limit",
        ));
    }
    *line = String::from_utf8(bytes)
        .map_err(|_error| io::Error::new(io::ErrorKind::InvalidData, "agent socket response is not UTF-8"))?;
    Ok(Some(read))
}

fn exit_code_u8(code: ExitCode) -> u8 {
    u8::from(code != ExitCode::SUCCESS)
}

#[cfg(test)]
const MAX_BUFFERED_AGENT_RESPONSE_BYTES: usize = MAX_SOCKET_FRAME_BYTES * 4;
#[cfg(test)]
const MAX_BUFFERED_AGENT_RENDERED_BYTES: usize = MAX_SOCKET_FRAME_BYTES;
#[cfg(test)]
const MAX_BUFFERED_AGENT_EVENTS: usize = 8192;
#[cfg(test)]
const MAX_BUFFERED_AGENT_DIAGNOSTICS: usize = 1024;

#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
struct BufferedAgentEvents {
    output: String,
    diagnostics: Vec<String>,
    exit_code: u8,
    interrupted: bool,
}

#[cfg(test)]
fn collect_agent_events_buffered(reader: impl BufRead) -> Result<BufferedAgentEvents, CliError> {
    collect_agent_events_buffered_with(reader, None)
}

#[cfg(test)]
fn collect_agent_events_buffered_interruptible(
    reader: impl BufRead,
    interrupt: &AtomicBool,
) -> Result<BufferedAgentEvents, CliError> {
    collect_agent_events_buffered_with(reader, Some(interrupt))
}

#[cfg(test)]
fn collect_agent_events_buffered_with(
    mut reader: impl BufRead,
    interrupt: Option<&AtomicBool>,
) -> Result<BufferedAgentEvents, CliError> {
    let mut saw_delta = false;
    let mut usage_totals = AgentUsageTotals::default();
    let mut output = String::new();
    let mut diagnostics = Vec::new();
    let mut exit_code = 0;
    let mut response_bytes: usize = 0;
    let mut events = 0;
    let mut line = String::new();
    loop {
        line.clear();
        match read_agent_socket_event_line_limited(&mut reader, &mut line) {
            Ok(None) => break,
            Ok(Some(bytes)) => {
                response_bytes = response_bytes.checked_add(bytes).ok_or_else(|| {
                    CliError::unavailable("agent response exceeds buffered response limit")
                })?;
                if response_bytes > MAX_BUFFERED_AGENT_RESPONSE_BYTES {
                    return Err(CliError::unavailable(format!(
                        "agent response exceeds {MAX_BUFFERED_AGENT_RESPONSE_BYTES} buffered bytes"
                    )));
                }
            }
            Err(error)
                if interrupt.is_some()
                    && matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
            {
                if interrupt.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                    return Ok(BufferedAgentEvents {
                        output,
                        diagnostics,
                        exit_code,
                        interrupted: true,
                    });
                }
                continue;
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot read socket response: {error}"
                )));
            }
        }
        events += 1;
        if events > MAX_BUFFERED_AGENT_EVENTS {
            return Err(CliError::unavailable(format!(
                "agent response exceeds {MAX_BUFFERED_AGENT_EVENTS} buffered events"
            )));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            push_buffered_output(&mut output, line)?;
            push_buffered_output(&mut output, "\n")?;
            continue;
        };
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("delta" | "reasoning_delta") => {
                if let Some(text) = json_text_field(&value) {
                    push_buffered_output(&mut output, text)?;
                    saw_delta = true;
                }
            }
            Some("message")
                if value.get("role").and_then(serde_json::Value::as_str) == Some("tool") =>
            {
                push_buffered_diagnostic(&mut diagnostics, tool_result_diagnostic(&value))?;
            }
            Some("message" | "reasoning_message") if !saw_delta => {
                if let Some(text) = json_text_field(&value) {
                    push_buffered_output(&mut output, text)?;
                    push_buffered_output(&mut output, "\n")?;
                }
            }
            Some("tool_call") => {
                let name = value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("tool_call");
                push_buffered_diagnostic(&mut diagnostics, tool_running_diagnostic(name))?;
            }
            Some("usage") => {
                if let Some(diagnostic) = usage_totals.record_event(&value) {
                    push_buffered_diagnostic(&mut diagnostics, diagnostic)?;
                }
            }
            Some("error") => {
                let code = value
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("EIO");
                let message = value
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("runtime error");
                push_buffered_diagnostic(
                    &mut diagnostics,
                    error_diagnostic(code, message),
                )?;
                exit_code = 1;
            }
            Some("pong") => push_buffered_output(&mut output, "pong\n")?,
            Some("done") if saw_delta => {
                push_buffered_output(&mut output, "\n")?;
                saw_delta = false;
            }
            _ => {}
        }
    }
    Ok(BufferedAgentEvents {
        output,
        diagnostics,
        exit_code,
        interrupted: false,
    })
}

#[derive(Default)]
struct AgentUsageTotals {
    input_tokens: u64,
    output_tokens: u64,
}

impl AgentUsageTotals {
    fn record_event(&mut self, value: &serde_json::Value) -> Option<String> {
        let input_delta = value
            .get("input_tokens")
            .and_then(serde_json::Value::as_u64)?;
        let output_delta = value
            .get("output_tokens")
            .and_then(serde_json::Value::as_u64)?;
        self.input_tokens = self.input_tokens.saturating_add(input_delta);
        self.output_tokens = self.output_tokens.saturating_add(output_delta);
        let color = color_enabled();
        Some(format!(
            "{} {} {}",
            styled(color, ANSI_DIM, "tokens"),
            styled(color, ANSI_CYAN, &format!("in +{input_delta}/{}", self.input_tokens)),
            styled(color, ANSI_GREEN, &format!("out +{output_delta}/{}", self.output_tokens))
        ))
    }
}

fn waiting_diagnostic(seconds: u64) -> String {
    let color = color_enabled();
    format!(
        "{} {}",
        styled(color, ANSI_DIM, "waiting"),
        styled(color, ANSI_CYAN, &format!("{seconds}s for agent event"))
    )
}

fn debug_timing_diagnostic(value: &serde_json::Value) -> Option<String> {
    let elapsed = value
        .get("elapsed_ms")
        .and_then(serde_json::Value::as_u64)?;
    let stage = value
        .get("stage")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("event");
    let color = color_enabled();
    Some(format!(
        "{} {} {}",
        styled(color, ANSI_DIM, "[debug timing]"),
        styled(color, ANSI_CYAN, &format!("+{elapsed}ms")),
        styled(color, ANSI_DIM, &terminal_safe_text(stage))
    ))
}

fn tool_running_diagnostic(name: &str) -> String {
    let color = color_enabled();
    let name = terminal_safe_text(name);
    format!(
        "{} {} {}",
        styled(color, ANSI_BOLD_YELLOW, "tool"),
        styled(color, ANSI_CYAN, &name),
        styled(color, ANSI_DIM, "running")
    )
}

fn tool_result_diagnostic(value: &serde_json::Value) -> String {
    let color = color_enabled();
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("tool");
    let name = terminal_safe_text(name);
    let bytes = tool_message_content_bytes(value);
    format!(
        "{} {} {} {}",
        styled(color, ANSI_BOLD_YELLOW, "tool"),
        styled(color, ANSI_CYAN, &name),
        styled(color, ANSI_GREEN, "done"),
        styled(color, ANSI_DIM, &format!("{bytes} bytes"))
    )
}

fn tool_message_content_bytes(value: &serde_json::Value) -> usize {
    match value.get("content") {
        Some(content) if content.is_string() => {
            content.as_str().map_or(0, str::len)
        }
        Some(content) if content.is_array() => content.as_array().map_or(0, |items| {
            items
                .iter()
                .map(|item| {
                    item.get("content")
                        .or_else(|| item.get("text"))
                        .and_then(serde_json::Value::as_str)
                        .map_or_else(|| item.to_string().len(), str::len)
                })
                .sum()
        }),
        Some(other) => other.to_string().len(),
        None => 0,
    }
}

fn error_diagnostic(code: &str, message: &str) -> String {
    let color = color_enabled();
    let code = terminal_safe_text(code);
    let message = terminal_safe_text(message);
    format!(
        "{} {}: {}",
        styled(color, ANSI_RED, "error"),
        styled(color, ANSI_BOLD_YELLOW, &code),
        message
    )
}

#[cfg(test)]
fn push_buffered_output(output: &mut String, text: &str) -> Result<(), CliError> {
    let bytes = output.len().checked_add(text.len()).ok_or_else(|| {
        CliError::unavailable("agent output exceeds buffered output limit")
    })?;
    if bytes > MAX_BUFFERED_AGENT_RENDERED_BYTES {
        return Err(CliError::unavailable(format!(
            "agent output exceeds {MAX_BUFFERED_AGENT_RENDERED_BYTES} buffered bytes"
        )));
    }
    output.push_str(text);
    Ok(())
}

#[cfg(test)]
fn push_buffered_diagnostic(
    diagnostics: &mut Vec<String>,
    diagnostic: String,
) -> Result<(), CliError> {
    if diagnostics.len() >= MAX_BUFFERED_AGENT_DIAGNOSTICS {
        return Err(CliError::unavailable(format!(
            "agent response exceeds {MAX_BUFFERED_AGENT_DIAGNOSTICS} buffered diagnostics"
        )));
    }
    diagnostics.push(diagnostic);
    Ok(())
}

fn json_text_field(value: &serde_json::Value) -> Option<&str> {
    value
        .get("text")
        .or_else(|| value.get("content"))
        .and_then(serde_json::Value::as_str)
}

fn print_terminal_text(text: &str) -> Result<(), CliError> {
    let text = terminal_safe_text(text);
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))
}

fn print_terminal_line(line: &str) -> Result<(), CliError> {
    let line = terminal_safe_text(line);
    print_line(&line)
}

fn write_terminal_error(line: &str) -> Result<(), CliError> {
    let line = terminal_safe_text(line);
    write_error(&line).map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
}

fn write_terminal_diagnostic(line: &str) -> Result<(), CliError> {
    write_error(line).map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
}

fn terminal_safe_text(text: &str) -> String {
    let mut safe = String::with_capacity(text.len());
    for character in text.chars() {
        if is_terminal_safe_character(character) {
            safe.push(character);
        } else {
            safe.extend(character.escape_default());
        }
    }
    safe
}

fn is_terminal_safe_character(character: char) -> bool {
    !character.is_control() || matches!(character, '\n' | '\r' | '\t')
}

fn request_id() -> Result<String, CliError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CliError::unavailable(format!("system clock before epoch: {error}")))?
        .as_millis();
    Ok(format!("ctx-{millis}"))
}

fn agent_session_dir(root: &Path, agent: &str, session: Option<&str>) -> Result<PathBuf, CliError> {
    let session = agent_session_name(root, agent, session)?;
    Ok(ctx_home(root)?
        .join("agent")
        .join(agent)
        .join("session")
        .join(session))
}

fn agent_session_name(root: &Path, agent: &str, session: Option<&str>) -> Result<String, CliError> {
    require_cli_name("agent name", agent)?;
    if let Some(session) = session {
        require_cli_name("session name", session)?;
    }

    let session_root = ctx_home(root)?.join("agent").join(agent).join("session");
    Ok(match session {
        Some(name) => name.to_owned(),
        None => current_session_name(&session_root)?,
    })
}

fn agent_socket_path(root: &Path, agent: &str) -> Result<PathBuf, CliError> {
    require_cli_name("agent name", agent)?;
    Ok(root.join("agent").join(format!("{agent}.sock")))
}

fn require_cli_name(label: &str, value: &str) -> Result<(), CliError> {
    if is_object_name(value) {
        Ok(())
    } else {
        Err(CliError::usage(format!("invalid {label}: {value}")))
    }
}

fn object_socket_path(root: &Path, path: &str) -> Result<PathBuf, CliError> {
    let abi_path = classify_input_path(root, path)?;
    if !matches!(
        classify_abi_path(&abi_path),
        "ctx.model.exec" | "ctx.agent.exec"
    ) {
        return Err(CliError::usage(format!(
            "socket command requires model/NAME or agent/NAME: {path}"
        )));
    }

    let Some((class, name)) = abi_path.split_once('/') else {
        return Err(CliError::usage(format!("invalid object path: {path}")));
    };
    Ok(root.join(class).join(format!("{name}.sock")))
}

fn current_session_name(session_root: &Path) -> Result<String, CliError> {
    let current_path = session_root.join("index").join("current");
    match read_current_session_file(&current_path) {
        Ok(value) => {
            let session = value.trim();
            if is_object_name(session) {
                Ok(session.to_owned())
            } else {
                Err(CliError::unavailable(format!(
                    "invalid current session in {}",
                    current_path.display()
                )))
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
            ) =>
        {
            Ok("default".to_owned())
        }
        Err(error) => Err(CliError::unavailable(format!(
            "cannot read {}: {error}",
            current_path.display()
        ))),
    }
}

fn read_current_session_file(path: &Path) -> io::Result<String> {
    let mut file = open_current_session_plain_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "current session file is not a bounded regular file",
        ));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_current_session_plain_file(path: &Path) -> io::Result<fs::File> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "session file has no parent"))?;
    let parent_dir = open_read_dir_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid session file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(io::Error::from)?;
    Ok(fs::File::from(file_fd))
}

fn ctx_home(root: &Path) -> Result<PathBuf, CliError> {
    if let Some(home) = env::var_os("CTX_HOME") {
        return Ok(PathBuf::from(home));
    }

    Ok(root.join("home").join(current_uid()?))
}

fn current_uid() -> Result<String, CliError> {
    let output = id_command()
        .output()
        .map_err(|error| CliError::unavailable(format!("cannot run id -u: {error}")))?;
    if !output.status.success() {
        return Err(CliError::unavailable("id -u failed"));
    }
    let uid = String::from_utf8(output.stdout)
        .map_err(|_error| CliError::unavailable("id -u returned non-UTF-8 output"))?;
    parse_current_uid(&uid)
}

fn parse_current_uid(output: &str) -> Result<String, CliError> {
    let uid = output.trim();
    if uid.is_empty() {
        return Err(CliError::unavailable("id -u returned empty output"));
    }
    if !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(CliError::unavailable("id -u returned invalid uid"));
    }
    Ok(uid.to_owned())
}

const ID_PROGRAM: &str = "/usr/bin/id";

fn get_id_program() -> &'static str {
    ID_PROGRAM
}

fn id_command() -> std::process::Command {
    let mut command = std::process::Command::new(get_id_program());
    command
        .arg("-u")
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    command
}

#[cfg(test)]
mod objects_socket_id_program_tests {
    use super::{get_id_program, id_command, parse_current_uid};

    #[test]
    fn get_id_program_returns_absolute_path() {
        assert_eq!(get_id_program(), "/usr/bin/id");
    }

    #[test]
    fn id_command_uses_clean_runtime_environment() {
        let command = id_command();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut envs = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<Vec<_>>();
        envs.sort();

        assert_eq!(command.get_program(), "/usr/bin/id");
        assert_eq!(args, vec!["-u".to_owned()]);
        assert_eq!(
            envs,
            vec![("PATH".to_owned(), Some("/usr/bin:/bin".to_owned()))]
        );
    }

    #[test]
    fn parse_current_uid_accepts_digits_only() {
        assert_eq!(parse_current_uid("1000\n"), Ok("1000".to_owned()));
        assert!(parse_current_uid("1000\n1001\n").is_err());
        assert!(parse_current_uid("user\n").is_err());
        assert!(parse_current_uid("\n").is_err());
    }
}
