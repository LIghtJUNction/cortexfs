use crate::*;

use cortexfs::runtime::terminal::{TerminalEvent, append_event, mark_state, next_sequence};

pub(crate) fn run_pty(config: &RunConfig) -> Result<ExitCode, CtxtermError> {
    let (mut control, generation) = cortexfs::runtime::terminal::broker::register_supervisor(
        &config.broker.agent,
        &config.broker.session,
        &config.broker.unit,
    )
    .map_err(broker_error)?;
    let pair = native_pty_system()
        .openpty(pty_size())
        .map_err(|error| CtxtermError::unavailable(format!("cannot open pty: {error}")))?;
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
    cortexfs::runtime::terminal::broker::activate_supervisor(&mut control, &generation)
        .map_err(broker_error)?;
    let mut child = pair
        .slave
        .spawn_command(pty_command(config)?)
        .map_err(|error| CtxtermError::unavailable(format!("cannot run command: {error}")))?;
    drop(pair.slave);
    let control_writer = Arc::clone(&writer);
    let output_clients = Arc::clone(&clients);
    let mut killer = child.clone_killer();
    thread::spawn(move || {
        if run_broker_control(control, &control_writer, &clients).is_err() {
            let _result = killer.kill();
        }
    });
    let events = env::var_os("CTX_TERMINAL_EVENTS").map(std::path::PathBuf::from);
    let output_events = events.clone();
    let mut sequence = events
        .as_deref()
        .map(next_sequence)
        .transpose()
        .map_err(event_error)?
        .unwrap_or(1);
    let output = thread::spawn(move || -> io::Result<u64> {
        let mut buffer = [0; 8192];
        loop {
            let read = reader.read(&mut buffer)?;
            let Some(chunk) = buffer.get(..read).filter(|chunk| !chunk.is_empty()) else {
                return Ok(sequence);
            };
            if let Some(path) = output_events.as_deref() {
                append_event(
                    path,
                    &TerminalEvent::output(sequence, cortexfs::current_time_unix(), chunk),
                )
                .map_err(|error| io::Error::other(error.to_string()))?;
                sequence = sequence.saturating_add(1);
            }
            broadcast(&output_clients, chunk);
        }
    });
    let status = child
        .wait()
        .map_err(|error| CtxtermError::unavailable(format!("cannot wait for command: {error}")))?;
    sequence = output
        .join()
        .map_err(|_error| CtxtermError::unavailable("pty output thread failed"))?
        .map_err(|error| write_error_to_ctxterm(&error))?;
    if let Some(path) = events {
        append_event(
            &path,
            &TerminalEvent::exit(sequence, cortexfs::current_time_unix(), status.exit_code()),
        )
        .map_err(event_error)?;
        mark_state(&path, "exited").map_err(event_error)?;
    }
    Ok(exit_code(&status))
}

fn broker_error(error: impl std::fmt::Display) -> CtxtermError {
    CtxtermError::unavailable(format!("terminal broker failed: {error}"))
}

fn event_error(error: impl std::fmt::Display) -> CtxtermError {
    CtxtermError::unavailable(format!("terminal event failed: {error}"))
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
