use crate::*;

pub(crate) fn agent_terminal(
    root: &Path,
    name: &str,
    session: Option<&str>,
    write: bool,
) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", name)?;
    let session = agent_session_name(root, name, session)?;
    require_session_name(&session)?;
    let socket = agent_terminal_socket(root, name, &session)?;
    stream_terminal_socket(&socket, write, name, &session)
}

pub(crate) fn agent_terminal_socket(
    root: &Path,
    name: &str,
    session: &str,
) -> Result<PathBuf, CliError> {
    Ok(cortexfs_paths::session_terminal_from_home_path(
        &ctx_home(root)?,
        name,
        session,
        "main.sock",
    ))
}

pub(crate) fn terminal_socket_exists(socket: &Path) -> bool {
    let Some(parent) = socket.parent() else {
        return false;
    };
    let Ok(parent) = open_plain_directory(parent) else {
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

pub(crate) fn require_session_name(session: &str) -> Result<(), CliError> {
    if is_object_name(session) {
        Ok(())
    } else {
        Err(CliError::usage("invalid session name"))
    }
}

pub(crate) fn shell_quote_arg(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/' | b':' | b'@' | b'%' | b'+' | b'=' | b','))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub(crate) fn stream_terminal_socket(
    socket: &Path,
    write: bool,
    name: &str,
    session: &str,
) -> Result<ExitCode, CliError> {
    let mode = if write {
        cortexfs::runtime::terminal::broker::TerminalMode::Attach
    } else {
        cortexfs::runtime::terminal::broker::TerminalMode::Watch
    };
    let stream = cortexfs::runtime::terminal::broker::connect_terminal(name, session, mode)
        .map_err(|error| terminal_connect_cli_error(socket, name, session, &error))?;
    stream_terminal_stream(stream, write)
}

pub(crate) fn terminal_connect_cli_error(
    socket: &Path,
    name: &str,
    session: &str,
    error: &cortexfs::runtime::terminal::broker::BrokerProtocolError,
) -> CliError {
    use cortexfs::runtime::terminal::broker::BrokerProtocolError::{Io, Rejected};
    let reason = match *error {
        Io(ref error) => match error.kind() {
            io::ErrorKind::NotFound => "terminal is not running",
            io::ErrorKind::ConnectionRefused => "terminal socket exists but has no listener",
            _ => "cannot connect terminal",
        },
        Rejected(ref code, _) if code == "not_ready" => "terminal is not running",
        _ => "cannot connect terminal",
    };
    let hint = format!(
        "run: ctx agent start {} --session {}",
        shell_quote_arg(name),
        shell_quote_arg(session)
    );
    CliError::unavailable(format!("{reason} {}: {error}\n{hint}", socket.display()))
}

pub(crate) fn stream_terminal_stream(
    stream: UnixStream,
    write: bool,
) -> Result<ExitCode, CliError> {
    let mut reader = stream
        .try_clone()
        .map_err(|error| CliError::unavailable(format!("cannot clone terminal socket: {error}")))?;
    let output = std::thread::spawn(move || copy_reader_to_stdout(&mut reader));
    if write {
        let _raw_mode = RawTerminalMode::maybe_new().map_err(|error| {
            CliError::unavailable(format!("cannot enter raw terminal mode: {error}"))
        })?;
        let input = std::thread::spawn(move || copy_stdin_to_stream_and_shutdown(stream));
        match input.join() {
            Ok(Ok(_bytes)) => {}
            Ok(Err(error)) if is_terminal_disconnect(&error) => {}
            Ok(Err(error)) => {
                return Err(CliError::unavailable(format!(
                    "terminal input failed: {error}"
                )));
            }
            Err(_error) => return Err(CliError::unavailable("terminal input thread failed")),
        }
    }
    match output.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) if is_terminal_disconnect(&error) => {}
        Ok(Err(error)) => {
            return Err(CliError::unavailable(format!(
                "terminal output failed: {error}"
            )));
        }
        Err(_error) => return Err(CliError::unavailable("terminal output thread failed")),
    }
    Ok(ExitCode::SUCCESS)
}
