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
    let entries = fs::read_dir(dir).map_err(|error| {
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

fn resume(root: &Path, agent: &str, session: Option<&str>) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, agent, session)?;
    let request = format!(
        "{{\"op\":\"resume\",\"session\":{}}}\n",
        json_string(&session)
    );
    stream_socket_request(&agent_socket_path(root, agent)?, &request)
}

fn send(root: &Path, agent: &str, session: &str, input: &str) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", agent)?;
    require_cli_name("session name", session)?;

    let request = format!(
        "{{\"op\":\"send\",\"id\":{},\"session\":{},\"input\":{}}}\n",
        json_string(&request_id()?),
        json_string(session),
        json_string(input)
    );
    stream_socket_request(&agent_socket_path(root, agent)?, &request)
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

    if let Some((guard, cancel_request, run_id)) = interrupt {
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|error| {
                CliError::unavailable(format!("cannot configure interruptible socket: {error}"))
            })?;
        let interrupted = render_agent_events_buffered_interruptible(stream, guard.interrupted_flag())?;
        if interrupted {
            write_terminal_error(&format!("ctx: interrupt requested; cancelling run {run_id}"))?;
            let cancel_code = stream_agent_socket_request(socket, cancel_request, false)?;
            if cancel_code != ExitCode::SUCCESS {
                return Ok(cancel_code);
            }
        }
        return Ok(ExitCode::SUCCESS);
    }

    render_agent_events_buffered(stream)
}

fn render_agent_events(stream: UnixStream) -> Result<ExitCode, CliError> {
    let reader = io::BufReader::new(stream);
    let mut saw_delta = false;
    let mut exit = ExitCode::SUCCESS;
    for line in reader.lines() {
        let line = line.map_err(|error| {
            CliError::unavailable(format!("cannot read socket response: {error}"))
        })?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            print_line(&line)?;
            continue;
        };
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("delta" | "reasoning_delta") => {
                if let Some(text) = json_text_field(&value) {
                    print_terminal_text(text)?;
                    saw_delta = true;
                }
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
                write_terminal_error(&format!("[tool] {name}"))?;
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
                write_terminal_error(&format!("error: {code}: {message}"))?;
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
    Ok(exit)
}

fn render_agent_events_buffered(stream: UnixStream) -> Result<ExitCode, CliError> {
    let reader = io::BufReader::new(stream);
    let rendered = collect_agent_events_buffered(reader)?;
    if !rendered.output.is_empty() {
        print_terminal_text(&rendered.output)?;
    }
    for diagnostic in rendered.diagnostics {
        write_terminal_error(&diagnostic)?;
    }
    Ok(ExitCode::from(rendered.exit_code))
}

fn render_agent_events_buffered_interruptible(
    stream: UnixStream,
    interrupt: &AtomicBool,
) -> Result<bool, CliError> {
    let mut reader = io::BufReader::new(stream);
    let rendered = collect_agent_events_buffered_interruptible(&mut reader, interrupt)?;
    if !rendered.output.is_empty() {
        print_terminal_text(&rendered.output)?;
    }
    for diagnostic in rendered.diagnostics {
        write_terminal_error(&diagnostic)?;
    }
    Ok(rendered.interrupted)
}

#[derive(Debug, Eq, PartialEq)]
struct BufferedAgentEvents {
    output: String,
    diagnostics: Vec<String>,
    exit_code: u8,
    interrupted: bool,
}

fn collect_agent_events_buffered(reader: impl BufRead) -> Result<BufferedAgentEvents, CliError> {
    collect_agent_events_buffered_with(reader, None)
}

fn collect_agent_events_buffered_interruptible(
    reader: impl BufRead,
    interrupt: &AtomicBool,
) -> Result<BufferedAgentEvents, CliError> {
    collect_agent_events_buffered_with(reader, Some(interrupt))
}

fn collect_agent_events_buffered_with(
    mut reader: impl BufRead,
    interrupt: Option<&AtomicBool>,
) -> Result<BufferedAgentEvents, CliError> {
    let mut saw_delta = false;
    let mut output = String::new();
    let mut diagnostics = Vec::new();
    let mut exit_code = 0;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_bytes) => {}
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
        let line = line.trim_end_matches(['\r', '\n']);
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            output.push_str(line);
            output.push('\n');
            continue;
        };
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("delta" | "reasoning_delta") => {
                if let Some(text) = json_text_field(&value) {
                    output.push_str(text);
                    saw_delta = true;
                }
            }
            Some("message" | "reasoning_message") if !saw_delta => {
                if let Some(text) = json_text_field(&value) {
                    output.push_str(text);
                    output.push('\n');
                }
            }
            Some("tool_call") => {
                let name = value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("tool_call");
                diagnostics.push(format!("[tool] {name}"));
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
                diagnostics.push(format!("error: {code}: {message}"));
                exit_code = 1;
            }
            Some("pong") => output.push_str("pong\n"),
            Some("done") if saw_delta => {
                output.push('\n');
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

fn json_text_field(value: &serde_json::Value) -> Option<&str> {
    value
        .get("text")
        .or_else(|| value.get("content"))
        .and_then(serde_json::Value::as_str)
}

fn print_terminal_text(text: &str) -> Result<(), CliError> {
    let text = terminal_safe_text(text);
    io::stdout()
        .lock()
        .write_all(text.as_bytes())
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
    match fs::read_to_string(&current_path) {
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
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("default".to_owned()),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot read {}: {error}",
            current_path.display()
        ))),
    }
}

fn ctx_home(root: &Path) -> Result<PathBuf, CliError> {
    if let Some(home) = env::var_os("CTX_HOME") {
        return Ok(PathBuf::from(home));
    }

    Ok(root.join("home").join(current_uid()?))
}

fn current_uid() -> Result<String, CliError> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_err(|error| CliError::unavailable(format!("cannot run id -u: {error}")))?;
    if !output.status.success() {
        return Err(CliError::unavailable("id -u failed"));
    }
    let uid = String::from_utf8(output.stdout)
        .map_err(|_error| CliError::unavailable("id -u returned non-UTF-8 output"))?;
    let uid = uid.trim();
    if uid.is_empty() {
        return Err(CliError::unavailable("id -u returned empty output"));
    }
    Ok(uid.to_owned())
}
