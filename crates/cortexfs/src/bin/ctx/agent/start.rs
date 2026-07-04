fn agent_start(root: &Path, args: &AgentStartArgs) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", &args.name)?;
    require_session_name(&args.session)?;
    require_sandbox_cwd(&args.cwd)?;
    let cli_mounts = agent_start_mounts(args)?;
    for mount in &cli_mounts {
        require_agent_mount(mount)?;
    }
    let view = derive_agent_runtime_view(root, &args.name).map_err(|error| {
        CliError::unavailable(format!(
            "cannot derive agent runtime view for {}: {error:?}",
            args.name
        ))
    })?;
    let visible_socket = agent_terminal_socket(root, &args.name, &args.session)?;
    let socket = agent_runtime_socket(root, &args.name, &args.session)?;
    ensure_agent_terminal_socket(&visible_socket, &socket)?;
    let unit = agent_terminal_unit(&args.name, &args.session);
    reset_agent_terminal_unit(&unit);
    let command = agent_start_systemd_command(root, args, &cli_mounts, &view, &socket, &unit);
    let output = agent_start_process_command(&command)
        .output()
        .map_err(|error| CliError::unavailable(format!("cannot start systemd-run: {error}")))?;
    if !output.status.success() {
        let diagnostics = systemd_run_diagnostics(&output);
        return Err(CliError::unavailable(format!(
            "agent terminal service failed to start with {}{diagnostics}",
            output.status
        )));
    }
    wait_for_agent_terminal_socket(&socket)?;
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
    let identity_lines = [("UID", uid.as_str()), ("GID", gid.as_str()), ("Groups", groups.as_str())];
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
        &visible_socket,
        &socket,
        &current_uid,
    ) {
        print_line(&line)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn record_agent_start_state(
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

fn agent_start_log_event(
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
fn agent_start_status_lines(
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

fn systemd_run_invocation_id(output: &std::process::Output) -> Option<String> {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.lines().find_map(|line| {
        line.rsplit_once("invocation ID: ")
            .map(|(_prefix, value)| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn systemd_run_diagnostics(output: &std::process::Output) -> String {
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

fn reset_agent_terminal_unit(unit: &str) {
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

fn agent_unit_main_pid(unit: &str) -> Option<String> {
    let service = format!("{unit}.service");
    let output = systemctl_user_command(["show", "--property", "MainPID", "--value", &service])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_systemctl_main_pid(&String::from_utf8_lossy(&output.stdout))
}

fn parse_systemctl_main_pid(output: &str) -> Option<String> {
    let pid = output.trim();
    if pid != "0" && pid.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(pid.to_owned())
    } else {
        None
    }
}

const SYSTEMCTL_PROGRAM: &str = "/usr/bin/systemctl";

fn get_systemctl_program() -> &'static str {
    SYSTEMCTL_PROGRAM
}

fn systemctl_user_command<const N: usize>(args: [&str; N]) -> ProcessCommand {
    let mut command = ProcessCommand::new(get_systemctl_program());
    command
        .arg("--user")
        .args(args);
    set_user_systemd_client_env(&mut command);
    command
}
