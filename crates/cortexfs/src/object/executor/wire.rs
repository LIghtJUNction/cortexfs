use super::*;

pub(crate) struct AgentModelStdoutReader {
    pub(crate) receiver: std::sync::mpsc::Receiver<Result<String, String>>,
    pub(crate) handle: thread::JoinHandle<()>,
}

pub(crate) fn spawn_agent_model_stdout_reader(
    stdout: std::process::ChildStdout,
) -> AgentModelStdoutReader {
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        loop {
            match read_agent_model_frame_line(&mut stdout) {
                Ok(Some(line)) => {
                    if sender.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ignored = sender.send(Err(error));
                    break;
                }
            }
        }
    });
    AgentModelStdoutReader { receiver, handle }
}

pub(crate) fn spawn_with_etxtbsy_retry(command: &mut Command) -> io::Result<Child> {
    for _attempt in 0..4 {
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    command.spawn()
}

pub(crate) fn read_agent_model_frame_line(
    reader: &mut impl BufRead,
) -> Result<Option<String>, String> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_AGENT_MODEL_FRAME_BYTES.saturating_add(1))
        .map_err(|_error| "agent model output frame limit is invalid".to_owned())?;
    let read = reader
        .take(limit)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| format!("cannot read agent model output: {error}"))?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > MAX_AGENT_MODEL_FRAME_BYTES {
        return Err("agent model output frame exceeds byte limit".to_owned());
    }
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| format!("agent model output frame is not utf-8: {error}"))
}

pub(crate) fn pass_runtime_provider_secret_env(command: &mut Command) {
    for name in [
        "CTX_PROVIDER_SECRET_FD",
        "CTX_PROVIDER_SECRET_PATH",
        "CTX_PROVIDER_SECRET_VALUE",
        "CTX_PROVIDER_SECRET_PROVIDER",
        "CTX_PROVIDER_SECRET_SLOT",
        "CTX_PROVIDER_CONFIG_DIR",
    ] {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
}

pub(crate) fn spawn_child_stderr_reader(
    mut stderr: std::process::ChildStderr,
) -> thread::JoinHandle<String> {
    thread::spawn(move || read_limited_text(&mut stderr, MAX_CHILD_STDERR_BYTES))
}

pub(crate) fn collect_child_stderr(reader: Option<thread::JoinHandle<String>>) -> String {
    let Some(reader) = reader else {
        return String::new();
    };
    reader.join().unwrap_or_default()
}
