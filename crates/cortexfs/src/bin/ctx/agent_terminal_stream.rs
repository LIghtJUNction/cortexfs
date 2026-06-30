fn agent_terminal(
    root: &Path,
    name: &str,
    session: Option<&str>,
    write: bool,
) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", name)?;
    let session = agent_session_name(root, name, session)?;
    require_session_name(&session)?;
    let socket = agent_terminal_connect_socket(root, name, &session)?;
    stream_terminal_socket(&socket, write, name, &session)
}

fn agent_terminal_socket(root: &Path, name: &str, session: &str) -> Result<PathBuf, CliError> {
    Ok(ctx_home(root)?
        .join("agent")
        .join(name)
        .join("session")
        .join(session)
        .join("terminal")
        .join("main.sock"))
}

fn agent_terminal_connect_socket(
    root: &Path,
    name: &str,
    session: &str,
) -> Result<PathBuf, CliError> {
    for socket in [
        agent_terminal_socket(root, name, session)?,
        agent_runtime_socket(root, name, session)?,
        agent_legacy_runtime_socket(root, name, session)?,
    ] {
        if terminal_socket_exists(&socket) {
            return Ok(socket);
        }
    }
    agent_terminal_socket(root, name, session)
}

fn terminal_socket_exists(socket: &Path) -> bool {
    let Some(parent) = socket.parent() else {
        return false;
    };
    let Ok(parent) = open_agent_terminal_runtime_dir(parent) else {
        return false;
    };
    let Some(file_name) = socket.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    nix::sys::stat::fstatat(&parent, file_name, nix::fcntl::AtFlags::empty()).is_ok_and(|stat| {
        nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFSOCK)
    })
}

fn require_session_name(session: &str) -> Result<(), CliError> {
    if is_object_name(session) {
        Ok(())
    } else {
        Err(CliError::usage("invalid session name"))
    }
}

fn shell_quote_arg(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/' | b':' | b'@' | b'%' | b'+' | b'=' | b','))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn stream_terminal_socket(
    socket: &Path,
    write: bool,
    name: &str,
    session: &str,
) -> Result<ExitCode, CliError> {
    let stream = open_terminal_socket(socket)
        .map_err(|error| terminal_connect_cli_error(socket, name, session, &error))?;
    stream_terminal_stream(stream, write)
}

fn open_terminal_socket(socket: &Path) -> Result<UnixStream, io::Error> {
    UnixStream::connect(socket)
}

fn terminal_connect_cli_error(
    socket: &Path,
    name: &str,
    session: &str,
    error: &io::Error,
) -> CliError {
    let hint = format!(
        "run: ctx agent start {} --session {}",
        shell_quote_arg(name),
        shell_quote_arg(session)
    );
    let reason = match error.kind() {
        io::ErrorKind::NotFound => "terminal is not running",
        io::ErrorKind::ConnectionRefused => "terminal socket exists but has no listener",
        _ => "cannot connect terminal socket",
    };
    CliError::unavailable(format!("{reason} {}: {error}\n{hint}", socket.display()))
}

fn stream_terminal_stream(mut stream: UnixStream, write: bool) -> Result<ExitCode, CliError> {
    if write {
        stream.write_all(b"attach\n")
    } else {
        stream.write_all(b"watch\n")
    }
    .map_err(|error| CliError::unavailable(format!("cannot write terminal mode: {error}")))?;

    let mut reader = stream
        .try_clone()
        .map_err(|error| CliError::unavailable(format!("cannot clone terminal socket: {error}")))?;
    let output = std::thread::spawn(move || copy_reader_to_stdout(&mut reader));
    if write {
        let _raw_mode = RawTerminalMode::maybe_new().map_err(|error| {
            CliError::unavailable(format!("cannot enter raw terminal mode: {error}"))
        })?;
        let input = std::thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            let result = io::copy(&mut stdin, &mut stream);
            drop(stdin);
            let _ignored = stream.shutdown(Shutdown::Write);
            result
        });
        match input.join() {
            Ok(Ok(_bytes)) => {}
            Ok(Err(error)) if is_terminal_disconnect(&error) => {}
            Ok(Err(error)) => {
                return Err(CliError::unavailable(format!("terminal input failed: {error}")));
            }
            Err(_error) => return Err(CliError::unavailable("terminal input thread failed")),
        }
    }
    match output.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) if is_terminal_disconnect(&error) => {}
        Ok(Err(error)) => {
            return Err(CliError::unavailable(format!("terminal output failed: {error}")));
        }
        Err(_error) => return Err(CliError::unavailable("terminal output thread failed")),
    }
    Ok(ExitCode::SUCCESS)
}

fn copy_reader_to_stdout(mut reader: impl Read) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut buffer = [0; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let Some(chunk) = buffer.get(..read) else {
            return Err(io::Error::other("terminal output read exceeded buffer"));
        };
        stdout.write_all(chunk)?;
        stdout.flush()?;
    }
    Ok(())
}

fn is_terminal_disconnect(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
    )
}

#[derive(Debug)]
struct RawTerminalMode {
    original: Termios,
}

impl RawTerminalMode {
    fn maybe_new() -> io::Result<Option<Self>> {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            return Ok(None);
        }
        let original = tcgetattr(stdin.as_fd()).map_err(nix_error_to_io)?;
        let mut raw = original.clone();
        cfmakeraw(&mut raw);
        tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw).map_err(nix_error_to_io)?;
        Ok(Some(Self { original }))
    }
}

impl Drop for RawTerminalMode {
    fn drop(&mut self) {
        let stdin = io::stdin();
        let _ignored = tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &self.original);
    }
}

fn nix_error_to_io(error: nix::errno::Errno) -> io::Error {
    io::Error::from(error)
}

