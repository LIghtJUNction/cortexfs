use crate::*;

pub(crate) fn start_listener(
    socket: &Path,
    pty_writer: PtyWriter,
    clients: Clients,
) -> Result<(), CtxtermError> {
    let token_hash = env::var(CLIENT_TOKEN_HASH_ENV)
        .ok()
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .or_else(|| {
            env::var(CLIENT_TOKEN_ENV)
                .ok()
                .filter(|token| valid_client_token(token))
                .map(|token| token_hash(&token))
        });
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
    let Some(token_hash) = token_hash else {
        let _ignored = std::fs::remove_file(socket);
        return Err(CtxtermError::usage(
            "CTXTERM_TOKEN must contain a non-empty terminal capability",
        ));
    };
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            handle_client(stream, Arc::clone(&pty_writer), &clients, &token_hash);
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

pub(crate) fn handle_client(
    mut stream: UnixStream,
    pty_writer: PtyWriter,
    clients: &Clients,
    expected_hash: &str,
) {
    let Ok((mode, supplied)) = read_client_mode_with_timeout(&mut stream) else {
        return;
    };
    if !tokens_equal(expected_hash.as_bytes(), token_hash(&supplied).as_bytes()) {
        return;
    }
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

pub(crate) fn read_client_mode_with_timeout(
    stream: &mut UnixStream,
) -> io::Result<(ClientMode, String)> {
    read_client_mode_with_timeout_duration(stream, CLIENT_MODE_TIMEOUT)
}

pub(crate) fn read_client_mode_with_timeout_duration(
    stream: &mut UnixStream,
    timeout: Duration,
) -> io::Result<(ClientMode, String)> {
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

pub(crate) fn read_client_mode(stream: &mut UnixStream) -> io::Result<(ClientMode, String)> {
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
    let mode = match mode.as_slice() {
        b"watch" => ClientMode::Watch,
        b"attach" => ClientMode::Attach,
        b"emit" => ClientMode::Emit,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid ctxterm client mode",
            ));
        }
    };
    let token = read_client_token(stream)?;
    Ok((mode, token))
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
