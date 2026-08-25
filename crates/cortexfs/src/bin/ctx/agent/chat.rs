use crate::*;

#[derive(Clone, Copy)]
pub(crate) struct AgentSend<'a> {
    pub(crate) session: Option<&'a str>,
    pub(crate) input: &'a str,
    pub(crate) raw: bool,
    pub(crate) debug: bool,
    pub(crate) approvals: &'a [String],
}

pub(crate) fn agent_send(
    root: &Path,
    name: &str,
    send: AgentSend<'_>,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, send.session)?;
    let request = agent_send_request_json(
        &request_id()?,
        &session,
        &agent_cwd(root, name)?,
        send.input,
        send.debug,
    );
    stream_agent_socket_request_approving(
        &agent_chat_request_socket(root, name)?,
        &request,
        send.raw,
        send.approvals,
    )
}

pub(crate) fn agent_send_request_json(
    run_id: &str,
    session: &str,
    cwd: &str,
    input: &str,
    debug: bool,
) -> String {
    if debug {
        return format!(
            "{{\"op\":\"send\",\"id\":{},\"session\":{},\"scope\":\"private\",\"cwd\":{},\"input\":{},\"origin\":{{\"transport\":\"terminal\"}},\"debug\":true}}\n",
            json_string(run_id),
            json_string(session),
            json_string(cwd),
            json_string(input)
        );
    }
    format!(
        "{{\"op\":\"send\",\"id\":{},\"session\":{},\"scope\":\"private\",\"cwd\":{},\"input\":{},\"origin\":{{\"transport\":\"terminal\"}}}}\n",
        json_string(run_id),
        json_string(session),
        json_string(cwd),
        json_string(input)
    )
}

pub(crate) fn agent_resume(
    root: &Path,
    name: &str,
    session: Option<&str>,
    raw: bool,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, session)?;
    let request = format!(
        "{{\"op\":\"resume\",\"session\":{}}}\n",
        json_string(&session)
    );
    stream_agent_socket_request(&agent_chat_request_socket(root, name)?, &request, raw)
}

pub(crate) fn agent_cancel(
    root: &Path,
    name: &str,
    session: Option<&str>,
    run: Option<&str>,
    raw: bool,
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, session)?;
    let run = match run {
        Some(run) => run.to_owned(),
        None => latest_run_id(root, name, &session)?,
    };
    require_cli_name("run id", &run)?;
    let request = format!("{{\"op\":\"cancel\",\"id\":{}}}\n", json_string(&run));
    stream_agent_socket_request(&agent_chat_request_socket(root, name)?, &request, raw)
}

pub(crate) fn agent_chat_request_socket(root: &Path, name: &str) -> Result<PathBuf, CliError> {
    let runtime_socket = agent_chat_runtime_socket(root, name)?;
    if terminal_socket_exists(&runtime_socket) {
        return Ok(runtime_socket);
    }
    let visible_socket = agent_socket_path(root, name)?;
    if terminal_socket_exists(&visible_socket) {
        return Ok(visible_socket);
    }
    Ok(visible_socket)
}

pub(crate) fn agent_chat(
    root: &Path,
    name: &str,
    session: Option<&str>,
    raw: bool,
    approvals: &[String],
) -> Result<ExitCode, CliError> {
    let session = agent_session_name(root, name, session)?;
    let sibling = env::current_exe()
        .ok()
        .map(|path| path.with_file_name("ctxchat"));
    let program = sibling
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ctxchat"));
    let mut command = std::process::Command::new(program);
    command.args(ctxchat_args(root, name, &session, raw, approvals));
    let status = command
        .status()
        .map_err(|error| CliError::unavailable(format!("cannot start ctxchat: {error}")))?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(1), ExitCode::from))
}

fn ctxchat_args(
    root: &Path,
    name: &str,
    session: &str,
    raw: bool,
    approvals: &[String],
) -> Vec<String> {
    let mut args = vec![
        "--root".to_owned(),
        root.display().to_string(),
        name.to_owned(),
        "--session".to_owned(),
        session.to_owned(),
    ];
    if raw {
        args.push("--raw".to_owned());
    }
    for approval in approvals {
        args.push("--approval".to_owned());
        args.push(approval.clone());
    }
    args
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[test]
    fn adapter_forwards_raw_and_repeatable_approvals() {
        assert_eq!(
            ctxchat_args(
                Path::new("/ctx"),
                "executor",
                "work",
                true,
                &["example.echo".to_owned(), "fs.read".to_owned()],
            ),
            [
                "--root",
                "/ctx",
                "executor",
                "--session",
                "work",
                "--raw",
                "--approval",
                "example.echo",
                "--approval",
                "fs.read",
            ]
        );
    }
}
