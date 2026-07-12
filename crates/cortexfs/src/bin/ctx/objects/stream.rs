use crate::*;

fn open_socket_request(socket: &Path, request: &str) -> Result<UnixStream, CliError> {
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
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;
    Ok(stream)
}

pub(crate) fn stream_socket_request(socket: &Path, request: &str) -> Result<ExitCode, CliError> {
    let stream = open_socket_request(socket, request)?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;
    copy_socket_response_raw(stream)?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn stream_socket_request_interruptible(
    socket: &Path,
    request: &str,
    interrupt: &AtomicBool,
) -> Result<bool, CliError> {
    let stream = open_socket_request(socket, request)?;
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| {
            CliError::unavailable(format!("cannot configure interruptible socket: {error}"))
        })?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;

    copy_socket_response_interruptible(stream, interrupt)
}

pub(crate) fn copy_socket_response_interruptible(
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
                    return Err(CliError::unavailable(
                        "socket response read exceeded buffer",
                    ));
                };
                if let Err(error) = stdout.write_all(bytes) {
                    if is_stdout_disconnect(&error) {
                        return Ok(false);
                    }
                    return Err(CliError::unavailable(format!(
                        "stdout write failed: {error}"
                    )));
                }
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

pub(crate) fn copy_socket_response_raw(mut stream: UnixStream) -> Result<(), CliError> {
    let mut stdout = io::stdout().lock();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream.read(&mut buffer).map_err(|error| {
            CliError::unavailable(format!("cannot read socket response: {error}"))
        })?;
        if read == 0 {
            return Ok(());
        }
        let Some(bytes) = buffer.get(..read) else {
            return Err(CliError::unavailable(
                "socket response read exceeded buffer",
            ));
        };
        if let Err(error) = stdout.write_all(bytes) {
            if is_stdout_disconnect(&error) {
                return Ok(());
            }
            return Err(CliError::unavailable(format!(
                "stdout write failed: {error}"
            )));
        }
    }
}

pub(crate) fn is_stdout_disconnect(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::BrokenPipe)
}

pub(crate) fn stream_agent_socket_request(
    socket: &Path,
    request: &str,
    raw: bool,
) -> Result<ExitCode, CliError> {
    if raw {
        return stream_socket_request(socket, request);
    }
    let stream = open_socket_request(socket, request)?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| CliError::unavailable(format!("cannot write socket request: {error}")))?;
    render_agent_events(stream)
}

pub(crate) fn stream_agent_socket_request_approving(
    socket: &Path,
    request: &str,
    raw: bool,
    approvals: &[String],
) -> Result<ExitCode, CliError> {
    if raw || approvals.is_empty() {
        return stream_agent_socket_request(socket, request, raw);
    }
    let stream = open_socket_request(socket, request)?;
    let writer = stream
        .try_clone()
        .map_err(|error| CliError::unavailable(format!("cannot clone approval socket: {error}")))?;
    render_agent_events_approving(stream, writer, approvals)
}

pub(crate) fn stream_agent_socket_request_streaming_interruptible(
    socket: &Path,
    request: &str,
    raw: bool,
    interrupt: Option<(&AgentInterruptGuard, &str, &str)>,
    approvals: &[String],
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
    let stream = open_socket_request(socket, request)?;
    if approvals.is_empty() {
        stream.shutdown(Shutdown::Write).map_err(|error| {
            CliError::unavailable(format!("cannot write socket request: {error}"))
        })?;
    }

    let Some((guard, cancel_request, run_id)) = interrupt else {
        if approvals.is_empty() {
            return render_agent_events(stream);
        }
        let writer = stream.try_clone().map_err(|error| {
            CliError::unavailable(format!("cannot clone approval socket: {error}"))
        })?;
        return render_agent_events_approving(stream, writer, approvals);
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| {
            CliError::unavailable(format!("cannot configure interruptible socket: {error}"))
        })?;
    let rendered =
        render_agent_events_interruptible_approving(stream, guard.interrupted_flag(), approvals)?;
    if rendered.interrupted {
        write_terminal_error(&format!(
            "ctx: interrupt requested; cancelling run {run_id}"
        ))?;
        send_socket_request_best_effort(socket, cancel_request)?;
    }
    Ok(ExitCode::from(rendered.exit_code))
}

pub(crate) fn send_socket_request_best_effort(
    socket: &Path,
    request: &str,
) -> Result<(), CliError> {
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
