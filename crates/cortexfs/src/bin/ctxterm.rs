use std::env;
use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, IsTerminal, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsFd;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::thread;

use nix::sys::termios::{SetArg, Termios, cfmakeraw, tcgetattr, tcsetattr};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_SHELL: &str = "tsh";
const CLIENT_MODE_LIMIT: usize = 16;

type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type Client = Arc<Mutex<UnixStream>>;
type Clients = Arc<Mutex<Vec<Client>>>;

#[derive(Debug, Eq, PartialEq)]
struct CtxtermError {
    code: u8,
    message: String,
}

impl CtxtermError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: 69,
            message: message.into(),
        }
    }
}

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            let _ignored = write_error(&format!("ctxterm: {}", error.message));
            ExitCode::from(error.code)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<ExitCode, CtxtermError> {
    let command = parse_args(args)?;
    match command {
        CtxtermCommand::Help => print_help().map(|()| ExitCode::SUCCESS),
        CtxtermCommand::Run {
            listen,
            log,
            stdio,
            program,
            args,
        } => run_pty(RunConfig {
            listen,
            log,
            stdio,
            program,
            args,
        }),
        CtxtermCommand::Client { socket, write } => run_client(&socket, write),
    }
}

#[derive(Debug, Eq, PartialEq)]
enum CtxtermCommand {
    Help,
    Run {
        listen: Option<PathBuf>,
        log: Option<PathBuf>,
        stdio: bool,
        program: OsString,
        args: Vec<OsString>,
    },
    Client {
        socket: PathBuf,
        write: bool,
    },
}

struct RunConfig {
    listen: Option<PathBuf>,
    log: Option<PathBuf>,
    stdio: bool,
    program: OsString,
    args: Vec<OsString>,
}

fn parse_args(args: Vec<OsString>) -> Result<CtxtermCommand, CtxtermError> {
    let mut values = args.into_iter();
    let mut listen = None;
    let mut log = None;
    let mut stdio = true;
    let Some(mut first) = values.next() else {
        return Ok(CtxtermCommand::Run {
            listen: None,
            log: None,
            stdio: true,
            program: OsString::from(DEFAULT_SHELL),
            args: Vec::new(),
        });
    };
    if first == "watch" || first == "attach" {
        let write = first == "attach";
        let Some(socket) = values.next() else {
            return Err(CtxtermError::usage("watch/attach requires a socket path"));
        };
        if let Some(extra) = values.next() {
            return Err(CtxtermError::usage(format!(
                "unexpected argument: {}",
                extra.to_string_lossy()
            )));
        }
        return Ok(CtxtermCommand::Client {
            socket: PathBuf::from(socket),
            write,
        });
    }
    if first == "--help" || first == "-h" {
        return Ok(CtxtermCommand::Help);
    }
    while first == "--listen" || first == "--log" || first == "--no-stdio" {
        match first.to_str() {
            Some("--listen") => {
                let Some(path) = values.next() else {
                    return Err(CtxtermError::usage("--listen requires a socket path"));
                };
                listen = Some(PathBuf::from(path));
            }
            Some("--log") => {
                let Some(path) = values.next() else {
                    return Err(CtxtermError::usage("--log requires a path"));
                };
                log = Some(PathBuf::from(path));
            }
            Some("--no-stdio") => {
                stdio = false;
            }
            _ => {}
        }
        let Some(next) = values.next() else {
            return Ok(CtxtermCommand::Run {
                listen,
                log,
                stdio,
                program: OsString::from(DEFAULT_SHELL),
                args: Vec::new(),
            });
        };
        first = next;
    }
    if first == "--" {
        let Some(program) = values.next() else {
            return Err(CtxtermError::usage("-- requires a command"));
        };
        return Ok(CtxtermCommand::Run {
            listen,
            log,
            stdio,
            program,
            args: values.collect(),
        });
    }
    Ok(CtxtermCommand::Run {
        listen,
        log,
        stdio,
        program: first,
        args: values.collect(),
    })
}

fn print_help() -> Result<(), CtxtermError> {
    write_stdout(
        "\
ctxterm - CortexFS agent terminal emulator

usage:
  ctxterm
  ctxterm --listen SOCKET [--log PATH] [--no-stdio] [-- COMMAND [ARG...]]
  ctxterm -- COMMAND [ARG...]
  ctxterm watch SOCKET
  ctxterm attach SOCKET

default:
  ctxterm starts tsh
",
    )
}

fn run_pty(config: RunConfig) -> Result<ExitCode, CtxtermError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size())
        .map_err(|error| CtxtermError::unavailable(format!("cannot open pty: {error}")))?;
    let mut command = CommandBuilder::new(&config.program);
    let cwd = env::current_dir().map_err(|error| {
        CtxtermError::unavailable(format!("cannot read current directory: {error}"))
    })?;
    command.cwd(cwd.as_os_str());
    command.args(config.args);
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

fn start_listener(
    socket: &Path,
    pty_writer: PtyWriter,
    clients: Clients,
) -> Result<(), CtxtermError> {
    if let Some(parent) = socket.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CtxtermError::unavailable(format!("cannot create {}: {error}", parent.display()))
        })?;
    }
    remove_stale_socket(socket).map_err(|error| {
        CtxtermError::unavailable(format!("cannot replace {}: {error}", socket.display()))
    })?;
    let listener = UnixListener::bind(socket).map_err(|error| {
        CtxtermError::unavailable(format!("cannot listen on {}: {error}", socket.display()))
    })?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600)).map_err(|error| {
        CtxtermError::unavailable(format!("cannot chmod {}: {error}", socket.display()))
    })?;
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            handle_client(stream, Arc::clone(&pty_writer), &clients);
        }
    });
    Ok(())
}

fn remove_stale_socket(socket: &Path) -> io::Result<()> {
    match fs::symlink_metadata(socket) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(socket),
        Ok(_metadata) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to replace non-socket path",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn handle_client(mut stream: UnixStream, pty_writer: PtyWriter, clients: &Clients) {
    let Ok(mode) = read_client_mode(&mut stream) else {
        return;
    };
    let Ok(output) = stream.try_clone() else {
        return;
    };
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClientMode {
    Watch,
    Attach,
}

fn read_client_mode(stream: &mut UnixStream) -> io::Result<ClientMode> {
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
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid ctxterm client mode",
        )),
    }
}

fn open_log(path: &Path) -> Result<fs::File, CtxtermError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CtxtermError::unavailable(format!("cannot create {}: {error}", parent.display()))
        })?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            CtxtermError::unavailable(format!("cannot open {}: {error}", path.display()))
        })
}

fn copy_stdin_to_pty(pty_writer: &PtyWriter) -> io::Result<()> {
    let stdin = io::stdin();
    let stdin = stdin.lock();
    copy_reader_to_pty(stdin, pty_writer)
}

fn copy_stream_to_pty(stream: UnixStream, pty_writer: &PtyWriter) -> io::Result<()> {
    copy_reader_to_pty(stream, pty_writer)
}

fn copy_reader_to_pty(mut reader: impl Read, pty_writer: &PtyWriter) -> io::Result<()> {
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

fn copy_reader_to_stdout(mut reader: impl Read) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut buffer = [0; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let Some(chunk) = buffer.get(..read) else {
            return Err(io::Error::other("output read exceeded buffer"));
        };
        stdout.write_all(chunk)?;
        stdout.flush()?;
    }
    Ok(())
}

fn broadcast(clients: &Clients, chunk: &[u8]) {
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

fn run_client(socket: &Path, write: bool) -> Result<ExitCode, CtxtermError> {
    let mut stream = UnixStream::connect(socket).map_err(|error| {
        CtxtermError::unavailable(format!("cannot connect {}: {error}", socket.display()))
    })?;
    if write {
        stream.write_all(b"attach\n")
    } else {
        stream.write_all(b"watch\n")
    }
    .map_err(|error| CtxtermError::unavailable(format!("cannot write client mode: {error}")))?;
    let mut reader = stream
        .try_clone()
        .map_err(|error| CtxtermError::unavailable(format!("cannot clone socket: {error}")))?;
    let output = thread::spawn(move || copy_reader_to_stdout(&mut reader));
    if write {
        let _raw_mode =
            RawTerminalMode::maybe_new().map_err(|error| write_error_to_ctxterm(&error))?;
        let input = thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            let result = io::copy(&mut stdin, &mut stream);
            drop(stdin);
            let _ignored = stream.shutdown(Shutdown::Write);
            result
        });
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

fn pty_size() -> PtySize {
    PtySize {
        rows: env_u16("LINES").unwrap_or(DEFAULT_ROWS),
        cols: env_u16("COLUMNS").unwrap_or(DEFAULT_COLS),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn env_u16(name: &str) -> Option<u16> {
    env::var(name).ok()?.parse::<u16>().ok()
}

fn exit_code(status: &portable_pty::ExitStatus) -> ExitCode {
    u8::try_from(status.exit_code()).map_or_else(|_error| ExitCode::from(1), ExitCode::from)
}

fn write_stdout(message: &str) -> Result<(), CtxtermError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(message.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| write_error_to_ctxterm(&error))
}

fn write_error(message: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{message}")
}

fn write_error_to_ctxterm(error: &io::Error) -> CtxtermError {
    CtxtermError::unavailable(format!("cannot write output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        ClientMode, Clients, CtxtermCommand, PtyWriter, parse_args, read_client_mode,
        remove_stale_socket, start_listener,
    };
    use std::ffi::OsString;
    use std::fs;
    use std::io::{self, Read, Write};
    use std::net::Shutdown;
    use std::os::unix::fs::symlink;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn ctxterm_defaults_to_tsh() {
        assert_eq!(
            parse_args(Vec::new()),
            Ok(CtxtermCommand::Run {
                listen: None,
                log: None,
                stdio: true,
                program: OsString::from("tsh"),
                args: Vec::new()
            })
        );
    }

    #[test]
    fn ctxterm_accepts_explicit_command_after_separator() {
        assert_eq!(
            parse_args(vec![
                OsString::from("--"),
                OsString::from("tsh"),
                OsString::from("--list"),
            ]),
            Ok(CtxtermCommand::Run {
                listen: None,
                log: None,
                stdio: true,
                program: OsString::from("tsh"),
                args: vec![OsString::from("--list")]
            })
        );
    }

    #[test]
    fn ctxterm_parses_listen_and_clients() {
        assert_eq!(
            parse_args(vec![
                OsString::from("--listen"),
                OsString::from("/tmp/main.sock"),
                OsString::from("--no-stdio"),
                OsString::from("--"),
                OsString::from("tsh"),
            ]),
            Ok(CtxtermCommand::Run {
                listen: Some(PathBuf::from("/tmp/main.sock")),
                log: None,
                stdio: false,
                program: OsString::from("tsh"),
                args: Vec::new()
            })
        );
        assert_eq!(
            parse_args(vec![
                OsString::from("watch"),
                OsString::from("/tmp/main.sock"),
            ]),
            Ok(CtxtermCommand::Client {
                socket: PathBuf::from("/tmp/main.sock"),
                write: false,
            })
        );
        assert_eq!(
            parse_args(vec![
                OsString::from("attach"),
                OsString::from("/tmp/main.sock"),
            ]),
            Ok(CtxtermCommand::Client {
                socket: PathBuf::from("/tmp/main.sock"),
                write: true,
            })
        );
    }

    #[test]
    fn client_mode_requires_newline() -> Result<(), Box<dyn std::error::Error>> {
        let (mut client, mut server) = UnixStream::pair()?;
        client.write_all(b"watch")?;
        client.shutdown(Shutdown::Write)?;

        let Err(error) = read_client_mode(&mut server) else {
            return Err("unterminated mode must fail".into());
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("must end with newline"));
        Ok(())
    }

    #[test]
    fn client_mode_keeps_attach_payload_after_newline() -> Result<(), Box<dyn std::error::Error>> {
        let (mut client, mut server) = UnixStream::pair()?;
        client.write_all(b"attach\npayload")?;
        client.shutdown(Shutdown::Write)?;

        let mode = read_client_mode(&mut server)?;
        let mut payload = String::new();
        server.read_to_string(&mut payload)?;

        assert_eq!(mode, ClientMode::Attach);
        assert_eq!(payload, "payload");
        Ok(())
    }

    #[test]
    fn remove_stale_socket_refuses_symlink_without_touching_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("target.txt");
        let link = dir.path().join("session.sock");
        fs::write(&target, "keep me")?;
        symlink(&target, &link)?;

        let Err(error) = remove_stale_socket(&link) else {
            return Err("symlinks are refused".into());
        };

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&target)?, "keep me");
        assert!(link.is_symlink());
        Ok(())
    }

    #[test]
    fn remove_stale_socket_only_removes_socket_inodes() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("session.sock");
        let listener = UnixListener::bind(&socket)?;
        drop(listener);

        remove_stale_socket(&socket)?;

        assert!(!socket.exists());
        Ok(())
    }

    #[test]
    fn start_listener_refuses_symlink_listen_path() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("target.txt");
        let link = dir.path().join("session.sock");
        fs::write(&target, "keep me")?;
        symlink(&target, &link)?;
        let writer: PtyWriter = Arc::new(Mutex::new(Box::new(Vec::<u8>::new())));
        let clients: Clients = Arc::new(Mutex::new(Vec::new()));

        let Err(error) = start_listener(&link, writer, clients) else {
            return Err("symlinks are refused".into());
        };

        assert_eq!(error.code, 69);
        assert!(
            error
                .message
                .contains("refusing to replace non-socket path")
        );
        assert_eq!(fs::read_to_string(&target)?, "keep me");
        assert!(link.is_symlink());
        Ok(())
    }
}
