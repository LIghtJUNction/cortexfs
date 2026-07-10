use crate::*;

pub(crate) fn agent_start(root: &Path, args: &AgentStartArgs) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", &args.name)?;
    require_session_name(&args.session)?;
    require_sandbox_cwd(&args.cwd)?;
    let cli_mounts = agent_start_mounts(args)?;
    for mount in &cli_mounts {
        require_agent_mount(mount)?;
    }
    let absolute_root = absolute_existing_path(root).map_err(|error| {
        CliError::unavailable(format!(
            "cannot resolve CTX_ROOT {}: {error}",
            root.display()
        ))
    })?;
    let root = absolute_root.as_path();
    let view = derive_agent_runtime_view(root, &args.name).map_err(|error| {
        CliError::unavailable(format!(
            "cannot derive agent runtime view for {}: {error:?}",
            args.name
        ))
    })?;
    let session_cwd = agent_start_sandbox_cwd(args, &cli_mounts);
    let session_workspace = agent_start_workspace_source(&cli_mounts);
    ensure_agent_start_session(
        root,
        args,
        &view,
        &session_cwd,
        session_workspace.as_deref(),
    )?;
    let AgentStartServices {
        visible_socket,
        socket,
        unit,
        output,
    } = start_agent_runtime_services(root, args, &cli_mounts, &view)?;
    let invocation = systemd_run_invocation_id(&output);
    let life = agent_lifecycle_name(view.lifecycle());
    let role = agent_role_for_display(view.agent_name());
    let uid = view.identity().uid().to_string();
    let gid = view.identity().gid().to_string();
    let groups = view
        .identity()
        .groups()
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let start_facts = [
        ("model", view.model()),
        ("life", life),
        ("role", role),
        ("uid", uid.as_str()),
        ("gid", gid.as_str()),
        ("groups", groups.as_str()),
    ];
    let identity_lines = [
        ("UID", uid.as_str()),
        ("GID", gid.as_str()),
        ("Groups", groups.as_str()),
    ];
    record_agent_start_state(root, args, &unit, &start_facts, invocation.as_deref())?;
    let current_uid = current_uid_for_ctx(root)?;
    for line in agent_start_status_lines(
        color_enabled(),
        &args.name,
        view.model(),
        life,
        role,
        &identity_lines,
        &args.session,
        &unit,
        invocation.as_deref(),
        &args.cwd,
        session_workspace.as_deref(),
        &visible_socket,
        &socket,
        &current_uid,
    ) {
        print_line(&line)?;
    }
    Ok(ExitCode::SUCCESS)
}

struct AgentStartServices {
    visible_socket: PathBuf,
    socket: PathBuf,
    unit: String,
    output: std::process::Output,
}

fn start_agent_runtime_services(
    root: &Path,
    args: &AgentStartArgs,
    cli_mounts: &[AgentMount],
    view: &AgentRuntimeView,
) -> Result<AgentStartServices, CliError> {
    let visible_socket = agent_terminal_socket(root, &args.name, &args.session)?;
    let socket = agent_runtime_socket(root, &args.name, &args.session)?;
    let chat_visible_socket = agent_socket_path(root, &args.name)?;
    let chat_socket = agent_chat_runtime_socket(root, &args.name)?;
    let chat_unit = agent_chat_unit(root, &args.name);
    let unit = agent_terminal_unit(&args.name, &args.session);
    let terminal_alias_created = ensure_agent_terminal_socket(&visible_socket, &socket)?;
    let terminal_aliases = terminal_alias_created
        .then_some((visible_socket.as_path(), socket.as_path()))
        .into_iter()
        .collect::<Vec<_>>();
    reset_agent_terminal_unit(&unit);
    let command = agent_start_systemd_command(root, args, cli_mounts, view, &socket, &unit);
    let output = match agent_start_process_command(&command).output() {
        Ok(output) => output,
        Err(error) => {
            rollback_agent_start_resources(&unit, None, &terminal_aliases, &[socket.as_path()]);
            return Err(CliError::unavailable(format!(
                "cannot start systemd-run: {error}"
            )));
        }
    };
    if !output.status.success() {
        let diagnostics = systemd_run_diagnostics(&output);
        let error = CliError::unavailable(format!(
            "agent terminal service failed to start with {}{diagnostics}",
            output.status
        ));
        rollback_agent_start_resources(&unit, None, &terminal_aliases, &[socket.as_path()]);
        return Err(error);
    }
    if let Err(error) = wait_for_agent_terminal_socket(&socket) {
        rollback_agent_start_resources(&unit, None, &terminal_aliases, &[socket.as_path()]);
        return Err(error);
    }
    let chat_alias_state = match ensure_agent_chat_socket(&chat_visible_socket, &chat_socket) {
        Ok(state) => state,
        Err(error) => {
            rollback_agent_start_resources(&unit, None, &terminal_aliases, &[socket.as_path()]);
            return Err(error);
        }
    };
    reset_agent_chat_unit(&chat_unit);
    let chat_command =
        agent_chat_socket_systemd_command(root, &args.name, &chat_socket, &chat_unit);
    let chat_output = match agent_start_process_command(&chat_command).output() {
        Ok(output) => output,
        Err(error) => {
            rollback_agent_late_chat_start(
                (&unit, &chat_unit),
                (
                    &terminal_aliases,
                    &[socket.as_path(), chat_socket.as_path()],
                ),
                (&chat_visible_socket, &chat_socket, &chat_alias_state),
            );
            return Err(CliError::unavailable(format!(
                "cannot start agent chat socket: {error}"
            )));
        }
    };
    if !chat_output.status.success() {
        let diagnostics = systemd_run_diagnostics(&chat_output);
        let error = CliError::unavailable(format!(
            "agent chat socket failed to start with {}{diagnostics}",
            chat_output.status
        ));
        rollback_agent_late_chat_start(
            (&unit, &chat_unit),
            (
                &terminal_aliases,
                &[socket.as_path(), chat_socket.as_path()],
            ),
            (&chat_visible_socket, &chat_socket, &chat_alias_state),
        );
        return Err(error);
    }
    if let Err(error) = wait_for_agent_chat_socket(&chat_socket) {
        rollback_agent_late_chat_start(
            (&unit, &chat_unit),
            (
                &terminal_aliases,
                &[socket.as_path(), chat_socket.as_path()],
            ),
            (&chat_visible_socket, &chat_socket, &chat_alias_state),
        );
        return Err(error);
    }
    Ok(AgentStartServices {
        visible_socket,
        socket,
        unit,
        output,
    })
}

pub(crate) fn ensure_agent_start_session(
    root: &Path,
    args: &AgentStartArgs,
    view: &AgentRuntimeView,
    cwd: &str,
    workspace: Option<&str>,
) -> Result<(), CliError> {
    let session_root = ctx_home(root)?
        .join("agent")
        .join(&args.name)
        .join("session");
    ensure_durable_session_layout(
        &session_root,
        &args.session,
        cwd,
        Some(view.model()),
        SocketSessionScope::Private,
    )
    .map_err(|error| {
        CliError::unavailable(format!(
            "cannot prepare agent session {}: {error:?}",
            args.session
        ))
    })?;
    if let Some(workspace) = workspace {
        write_agent_session_plain(
            &session_root.join(&args.session).join("workspace"),
            &format!("{workspace}\n"),
        )?;
    }
    Ok(())
}

pub(crate) fn record_agent_start_state(
    root: &Path,
    args: &AgentStartArgs,
    unit: &str,
    facts: &[(&str, &str)],
    invocation: Option<&str>,
) -> Result<(), CliError> {
    let control = agent_control_dir(root, &args.name);
    let metadata = fs::symlink_metadata(&control).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", control.display()))
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(CliError::unavailable(format!(
            "agent control is not a plain directory: {}",
            control.display()
        )));
    }
    write_agent_control_plain(&control.join("status"), "ready\n")?;
    let pid = agent_unit_main_pid(unit).unwrap_or_default();
    write_agent_control_plain(&control.join("pid"), &format!("{pid}\n"))?;
    append_agent_log_event(
        &control.join("log"),
        &agent_start_log_event(&args.name, &args.session, unit, facts, invocation),
    )
}

pub(crate) fn agent_start_log_event(
    agent: &str,
    session: &str,
    unit: &str,
    facts: &[(&str, &str)],
    invocation: Option<&str>,
) -> String {
    let mut fields = vec![
        r#""type":"agent.start""#.to_owned(),
        format!(r#""agent":{}"#, json_string(agent)),
        format!(r#""session":{}"#, json_string(session)),
        format!(r#""unit":{}"#, json_string(unit)),
        r#""status":"ready""#.to_owned(),
    ];
    for &(key, value) in facts {
        fields.insert(
            fields.len().saturating_sub(1),
            format!(r#""{key}":{}"#, json_string(value)),
        );
    }
    if let Some(invocation) = invocation {
        fields.push(format!(r#""invocation":{}"#, json_string(invocation)));
    }
    format!("{{{}}}", fields.join(","))
}

#[expect(
    clippy::too_many_arguments,
    reason = "status output mirrors systemctl's flat field list"
)]
pub(crate) fn agent_start_status_lines(
    color: bool,
    agent: &str,
    model: &str,
    life: &str,
    role: &str,
    identity_lines: &[(&str, &str)],
    session: &str,
    unit: &str,
    invocation: Option<&str>,
    cwd: &str,
    workspace: Option<&str>,
    visible_socket: &Path,
    runtime_socket: &Path,
    uid: &str,
) -> Vec<String> {
    let service = format!("{unit}.service");
    let loaded_path = format!("/run/user/{uid}/systemd/transient/{service}");
    let loaded = styled(color, ANSI_GREEN, "loaded");
    let mut lines = vec![
        format!(
            "{} {} - {}",
            styled(color, ANSI_BOLD_CYAN, "●"),
            styled(color, ANSI_BOLD_CYAN, &service),
            styled(color, ANSI_CYAN, "CortexFS agent terminal")
        ),
        format!(
            "     {} {} ({loaded_path}; transient)",
            styled(color, ANSI_BOLD_BLUE, "Loaded:"),
            loaded
        ),
        format!(
            "     {} {}",
            styled(color, ANSI_BOLD_BLUE, "Active:"),
            styled(color, ANSI_GREEN, "active (running)")
        ),
    ];
    if let Some(invocation) = invocation {
        lines.push(format!(
            " {} {}",
            styled(color, ANSI_BOLD_BLUE, "Invocation:"),
            styled(color, ANSI_DIM, invocation)
        ));
    }
    lines.extend([
        format!(
            "      {} {}",
            styled(color, ANSI_BOLD_BLUE, "Agent:"),
            styled(color, ANSI_CYAN, agent)
        ),
        format!(
            "      {} {}",
            styled(color, ANSI_BOLD_BLUE, "Model:"),
            styled(color, ANSI_CYAN, model)
        ),
        format!(
            "       {} {}",
            styled(color, ANSI_BOLD_BLUE, "Life:"),
            styled(color, ANSI_CYAN, life)
        ),
        format!(
            "       {} {}",
            styled(color, ANSI_BOLD_BLUE, "Role:"),
            styled(color, ANSI_CYAN, role)
        ),
    ]);
    for &(label, value) in identity_lines {
        lines.push(format!(
            "     {} {}",
            styled(color, ANSI_BOLD_BLUE, &format!("{label}:")),
            styled(color, ANSI_CYAN, value)
        ));
    }
    lines.extend([
        format!(
            "    {} {}",
            styled(color, ANSI_BOLD_BLUE, "Session:"),
            styled(color, ANSI_CYAN, session)
        ),
        format!(
            "        {} {}",
            styled(color, ANSI_BOLD_BLUE, "CWD:"),
            styled(color, ANSI_CYAN, cwd)
        ),
    ]);
    if let Some(workspace) = workspace {
        lines.push(format!(
            "  {} {}",
            styled(color, ANSI_BOLD_BLUE, "Workspace:"),
            styled(color, ANSI_CYAN, workspace)
        ));
    }
    lines.extend([
        format!(
            "     {} {}",
            styled(color, ANSI_BOLD_BLUE, "Socket:"),
            styled(color, ANSI_CYAN, &visible_socket.display().to_string())
        ),
        format!(
            " {} {}",
            styled(color, ANSI_BOLD_BLUE, "Runtime Socket:"),
            styled(color, ANSI_DIM, &runtime_socket.display().to_string())
        ),
    ]);
    lines
}

pub(crate) fn systemd_run_invocation_id(output: &std::process::Output) -> Option<String> {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.lines().find_map(|line| {
        line.rsplit_once("invocation ID: ")
            .map(|(_prefix, value)| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

pub(crate) fn systemd_run_diagnostics(output: &std::process::Output) -> String {
    let mut diagnostics = String::new();
    for bytes in [&output.stderr, &output.stdout] {
        let text = String::from_utf8_lossy(bytes);
        let text = text.trim();
        if !text.is_empty() {
            if diagnostics.is_empty() {
                diagnostics.push_str(": ");
            } else {
                diagnostics.push('\n');
            }
            diagnostics.push_str(text);
        }
    }
    diagnostics
}

pub(crate) fn reset_agent_terminal_unit(unit: &str) {
    let service = format!("{unit}.service");
    let _ignored = systemctl_user_command(["stop", &service])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ignored = systemctl_user_command(["reset-failed", &service])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub(crate) fn rollback_agent_start_resources(
    terminal_unit: &str,
    chat_unit: Option<&str>,
    aliases: &[(&Path, &Path)],
    runtime_sockets: &[&Path],
) {
    rollback_agent_start_resources_with(
        terminal_unit,
        chat_unit,
        aliases,
        runtime_sockets,
        reset_agent_terminal_unit,
        reset_agent_chat_unit,
    );
}

pub(crate) fn rollback_agent_start_resources_with(
    terminal_unit: &str,
    chat_unit: Option<&str>,
    aliases: &[(&Path, &Path)],
    runtime_sockets: &[&Path],
    reset_terminal: impl FnOnce(&str),
    reset_chat: impl FnOnce(&str),
) {
    if let Some(chat_unit) = chat_unit {
        reset_chat(chat_unit);
    }
    reset_terminal(terminal_unit);
    for &(visible, target) in aliases {
        remove_matching_socket_alias(visible, target);
    }
    for path in runtime_sockets {
        remove_plain_runtime_socket(path);
    }
}

type AgentStartAlias<'a> = (&'a Path, &'a Path);
type AgentStartCleanup<'a> = (&'a [AgentStartAlias<'a>], &'a [&'a Path]);
type AgentChatCleanup<'a> = (&'a Path, &'a Path, &'a AgentChatAliasState);

fn rollback_agent_late_chat_start(
    units: (&str, &str),
    terminal: AgentStartCleanup<'_>,
    chat: AgentChatCleanup<'_>,
) {
    rollback_agent_late_chat_start_with(
        units,
        terminal,
        chat,
        reset_agent_terminal_unit,
        reset_agent_chat_unit,
    );
}

pub(crate) fn rollback_agent_late_chat_start_with(
    units: (&str, &str),
    terminal: AgentStartCleanup<'_>,
    chat: AgentChatCleanup<'_>,
    reset_terminal: impl FnOnce(&str),
    reset_chat: impl FnOnce(&str),
) {
    rollback_agent_start_resources_with(
        units.0,
        Some(units.1),
        terminal.0,
        terminal.1,
        reset_terminal,
        reset_chat,
    );
    let _ignored = rollback_agent_chat_alias(chat.0, chat.1, chat.2);
}

fn remove_matching_socket_alias(visible: &Path, target: &Path) {
    let _ignored = remove_exact_socket_alias(visible, target);
}

fn remove_plain_runtime_socket(path: &Path) {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket()) {
        let _ignored = fs::remove_file(path);
    }
}

pub(crate) fn agent_unit_main_pid(unit: &str) -> Option<String> {
    let service = format!("{unit}.service");
    let output = systemctl_user_command(["show", "--property", "MainPID", "--value", &service])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_systemctl_main_pid(&String::from_utf8_lossy(&output.stdout))
}

pub(crate) fn parse_systemctl_main_pid(output: &str) -> Option<String> {
    let pid = output.trim();
    if pid != "0" && pid.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(pid.to_owned())
    } else {
        None
    }
}

const SYSTEMCTL_PROGRAM: &str = "/usr/bin/systemctl";

pub(crate) fn get_systemctl_program() -> &'static str {
    SYSTEMCTL_PROGRAM
}

pub(crate) fn systemctl_user_command<const N: usize>(args: [&str; N]) -> ProcessCommand {
    let mut command = ProcessCommand::new(get_systemctl_program());
    command.arg("--user").args(args);
    set_user_systemd_client_env(&mut command);
    command
}
