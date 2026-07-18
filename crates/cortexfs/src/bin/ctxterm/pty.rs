use crate::*;

pub(crate) fn run_pty(config: RunConfig) -> Result<ExitCode, CtxtermError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size())
        .map_err(|error| CtxtermError::unavailable(format!("cannot open pty: {error}")))?;
    let command = pty_command(&config)?;
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| CtxtermError::unavailable(format!("cannot run command: {error}")))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| CtxtermError::unavailable(format!("cannot open pty reader: {error}")))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| CtxtermError::unavailable(format!("cannot open pty writer: {error}")))?;
    let writer = Arc::new(Mutex::new(writer));
    let clients = Arc::new(Mutex::new(Vec::new()));
    let socket_path = config.listen.as_deref().map(Path::to_path_buf);
    if let Some(socket) = socket_path.as_deref() {
        start_listener(socket, Arc::clone(&writer), Arc::clone(&clients))?;
    }
    let log = match config.log {
        Some(path) => Some(open_log(&path)?),
        None => None,
    };

    let output_clients = Arc::clone(&clients);
    let output = thread::spawn(move || {
        let mut stdout = io::stdout().lock();
        let mut log = log;
        let mut buffer = [0; 8192];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let Some(chunk) = buffer.get(..read) else {
                return Err(io::Error::other("pty read exceeded buffer"));
            };
            if config.stdio {
                stdout.write_all(chunk)?;
                stdout.flush()?;
            }
            if let Some(file) = log.as_mut() {
                file.write_all(chunk)?;
                file.flush()?;
            }
            broadcast(&output_clients, chunk);
        }
        Ok(())
    });
    if config.stdio {
        let input_writer = Arc::clone(&writer);
        let _input = thread::spawn(move || copy_stdin_to_pty(&input_writer));
    }

    let status = child
        .wait()
        .map_err(|error| CtxtermError::unavailable(format!("cannot wait for command: {error}")))?;
    match output.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(write_error_to_ctxterm(&error)),
        Err(_error) => return Err(CtxtermError::unavailable("pty output thread failed")),
    }
    if let Some(socket) = socket_path.as_deref() {
        let _ignored = remove_stale_socket(socket);
    }
    Ok(exit_code(&status))
}

pub(crate) fn pty_command(config: &RunConfig) -> Result<CommandBuilder, CtxtermError> {
    pty_command_with_env(config, env::vars_os())
}

pub(crate) fn pty_command_with_env(
    config: &RunConfig,
    envs: impl IntoIterator<Item = (OsString, OsString)>,
) -> Result<CommandBuilder, CtxtermError> {
    let mut command = CommandBuilder::new(&config.program);
    command.env_clear();
    command.env("PATH", cortexfs::support::command::TRUSTED_PATH);
    command.env("TERM", "xterm-256color");
    for (key, value) in envs {
        if preserved_pty_env_key(&key) {
            command.env(key, value);
        }
    }
    let cwd = env::current_dir().map_err(|error| {
        CtxtermError::unavailable(format!("cannot read current directory: {error}"))
    })?;
    command.cwd(cwd.as_os_str());
    command.args(config.args.clone());
    Ok(command)
}

pub(crate) fn preserved_pty_env_key(key: &OsStr) -> bool {
    key.to_str()
        .is_some_and(|key| PRESERVED_PTY_ENV.contains(&key))
}
