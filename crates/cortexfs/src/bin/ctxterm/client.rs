use crate::*;

pub(crate) fn copy_stdin_to_pty(pty_writer: &PtyWriter) -> io::Result<()> {
    let stdin = io::stdin();
    let stdin = stdin.lock();
    copy_reader_to_pty(stdin, pty_writer)
}

pub(crate) fn copy_stream_to_pty(stream: UnixStream, pty_writer: &PtyWriter) -> io::Result<()> {
    copy_reader_to_pty(stream, pty_writer)
}

pub(crate) fn copy_reader_to_pty(mut reader: impl Read, pty_writer: &PtyWriter) -> io::Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let Some(chunk) = buffer.get(..read) else {
            return Err(io::Error::other("input read exceeded buffer"));
        };
        let mut writer = pty_writer
            .lock()
            .map_err(|_error| io::Error::other("pty writer lock poisoned"))?;
        writer.write_all(chunk)?;
        writer.flush()?;
    }
    Ok(())
}

pub(crate) fn broadcast(clients: &Clients, chunk: &[u8]) {
    let Ok(mut clients) = clients.lock() else {
        return;
    };
    clients.retain(|client| {
        let Ok(mut stream) = client.lock() else {
            return false;
        };
        stream
            .write_all(chunk)
            .and_then(|()| stream.flush())
            .is_ok()
    });
}

pub(crate) fn run_client(socket: &Path, write: bool) -> Result<ExitCode, CtxtermError> {
    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CtxtermError::unavailable(format!("cannot connect {}: {error}", socket.display()))
    })?;
    let token = env::var(CLIENT_TOKEN_ENV)
        .ok()
        .filter(|token| valid_client_token(token))
        .ok_or_else(|| {
            CtxtermError::usage("CTXTERM_TOKEN must contain a non-empty terminal capability")
        })?;
    let mode = if write { "attach" } else { "watch" };
    write!(stream, "{mode}\n{token}\n")
        .map_err(|error| CtxtermError::unavailable(format!("cannot write client mode: {error}")))?;
    let mut reader = stream
        .try_clone()
        .map_err(|error| CtxtermError::unavailable(format!("cannot clone socket: {error}")))?;
    let output = thread::spawn(move || copy_reader_to_stdout(&mut reader));
    if write {
        let _raw_mode =
            RawTerminalMode::maybe_new().map_err(|error| write_error_to_ctxterm(&error))?;
        let input = thread::spawn(move || copy_stdin_to_stream_and_shutdown(stream));
        match input.join() {
            Ok(Ok(_bytes)) => {}
            Ok(Err(error)) if is_terminal_disconnect(&error) => {}
            Ok(Err(error)) => return Err(write_error_to_ctxterm(&error)),
            Err(_error) => return Err(CtxtermError::unavailable("input thread failed")),
        }
    }
    match output.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) if is_terminal_disconnect(&error) => {}
        Ok(Err(error)) => return Err(write_error_to_ctxterm(&error)),
        Err(_error) => return Err(CtxtermError::unavailable("output thread failed")),
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn pty_size() -> PtySize {
    PtySize {
        rows: env_u16("LINES").unwrap_or(DEFAULT_ROWS),
        cols: env_u16("COLUMNS").unwrap_or(DEFAULT_COLS),
        pixel_width: 0,
        pixel_height: 0,
    }
}

pub(crate) fn env_u16(name: &str) -> Option<u16> {
    env_u16_from_value(env::var(name).ok().as_deref())
}

pub(crate) fn env_u16_from_value(value: Option<&str>) -> Option<u16> {
    value?.parse::<u16>().ok().filter(|value| *value > 0)
}

pub(crate) fn exit_code(status: &portable_pty::ExitStatus) -> ExitCode {
    u8::try_from(status.exit_code()).map_or_else(|_error| ExitCode::from(1), ExitCode::from)
}

pub(crate) fn write_stdout(message: &str) -> Result<(), CtxtermError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(message.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| write_error_to_ctxterm(&error))
}

pub(crate) fn write_error_to_ctxterm(error: &io::Error) -> CtxtermError {
    CtxtermError::unavailable(format!("cannot write output: {error}"))
}
