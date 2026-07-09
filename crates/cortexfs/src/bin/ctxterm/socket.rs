use crate::*;

pub(crate) fn start_listener(
    socket: &Path,
    pty_writer: PtyWriter,
    clients: Clients,
) -> Result<(), CtxtermError> {
    if let Some(parent) = socket.parent() {
        create_plain_directory(
            parent,
            0o700,
            "ctxterm parent path is not a plain directory",
            "ctxterm path contains a non-directory entry",
            "invalid ctxterm directory name",
        )
        .map_err(|error| {
            CtxtermError::unavailable(format!("cannot create {}: {error}", parent.display()))
        })?;
    }
    remove_stale_socket(socket).map_err(|error| {
        CtxtermError::unavailable(format!("cannot replace {}: {error}", socket.display()))
    })?;
    let listener = UnixListener::bind(socket).map_err(|error| {
        CtxtermError::unavailable(format!("cannot listen on {}: {error}", socket.display()))
    })?;
    set_ctxterm_socket_permissions(socket).map_err(|error| {
        CtxtermError::unavailable(format!("cannot chmod {}: {error}", socket.display()))
    })?;
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            handle_client(stream, Arc::clone(&pty_writer), &clients);
        }
    });
    Ok(())
}

pub(crate) fn set_ctxterm_socket_permissions(socket: &Path) -> io::Result<()> {
    let parent = socket.parent().unwrap_or_else(|| Path::new("."));
    let parent = open_plain_directory(parent)?;
    let file_name = socket
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid socket name"))?;
    nix::sys::stat::fchmodat(
        &parent,
        file_name,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
        nix::sys::stat::FchmodatFlags::NoFollowSymlink,
    )
    .map_err(io::Error::from)
}

pub(crate) fn handle_client(mut stream: UnixStream, pty_writer: PtyWriter, clients: &Clients) {
    let Ok(mode) = read_client_mode_with_timeout(&mut stream) else {
        return;
    };
    if mode == ClientMode::Emit {
        if let Ok(payload) = read_emit_payload(stream)
            && !payload.is_empty()
        {
            broadcast(clients, &payload);
        }
        return;
    }
    let Ok(output) = stream.try_clone() else {
        return;
    };
    if output
        .set_write_timeout(Some(CLIENT_WRITE_TIMEOUT))
        .is_err()
    {
        return;
    }
    let output = Arc::new(Mutex::new(output));
    if let Ok(mut clients) = clients.lock() {
        clients.push(output);
    }
    if mode == ClientMode::Attach {
        thread::spawn(move || {
            let _ignored = copy_stream_to_pty(stream, &pty_writer);
        });
    }
}

pub(crate) fn read_client_mode_with_timeout(stream: &mut UnixStream) -> io::Result<ClientMode> {
    read_client_mode_with_timeout_duration(stream, CLIENT_MODE_TIMEOUT)
}

pub(crate) fn read_client_mode_with_timeout_duration(
    stream: &mut UnixStream,
    timeout: Duration,
) -> io::Result<ClientMode> {
    stream.set_read_timeout(Some(timeout))?;
    let mode = read_client_mode(stream);
    stream.set_read_timeout(None)?;
    mode
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClientMode {
    Watch,
    Attach,
    Emit,
}

pub(crate) fn read_client_mode(stream: &mut UnixStream) -> io::Result<ClientMode> {
    let mut mode = Vec::new();
    let mut byte = [0; 1];
    let mut complete = false;
    while mode.len() <= CLIENT_MODE_LIMIT {
        let read = stream.read(&mut byte)?;
        if read == 0 {
            break;
        }
        if byte[0] == b'\n' {
            complete = true;
            break;
        }
        mode.push(byte[0]);
    }
    if !complete {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ctxterm client mode must end with newline",
        ));
    }
    match mode.as_slice() {
        b"watch" => Ok(ClientMode::Watch),
        b"attach" => Ok(ClientMode::Attach),
        b"emit" => Ok(ClientMode::Emit),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid ctxterm client mode",
        )),
    }
}

pub(crate) fn read_emit_payload(stream: UnixStream) -> io::Result<Vec<u8>> {
    let mut payload = Vec::new();
    let max_payload = u64::try_from(MAX_EMIT_PAYLOAD_BYTES)
        .map_err(|_error| io::Error::other("ctxterm payload limit overflow"))?;
    let mut reader = stream.take(max_payload.saturating_add(1));
    reader.read_to_end(&mut payload)?;
    if payload.len() > MAX_EMIT_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ctxterm emit payload too large",
        ));
    }
    Ok(payload)
}
