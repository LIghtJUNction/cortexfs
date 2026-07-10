fn agent_executable_socket_command(
    runtime: AgentExecutableSocketRuntime<'_>,
    agent_executable: &fs::File,
    request: AgentExecutableRunRequest<'_>,
) -> Command {
    match runtime.execution {
        AgentExecutableSocketExecution::Direct => {
            let mut command = Command::new(proc_fd_path(agent_executable));
            apply_agent_executable_socket_env(
                &mut command,
                runtime,
                request,
            );
            command.arg(request.input);
            command.stdout(Stdio::piped()).process_group(0);
            command
        }
        AgentExecutableSocketExecution::Bwrap {
            program,
            mount_table,
        } => {
            let mut command = Command::new(program);
            command.args(agent_executable_socket_bwrap_args(&BwrapAgentExecutableArgs {
                runtime,
                mount_table,
                cwd: request.cwd.unwrap_or(runtime.default_cwd),
                workspace: request.workspace,
                run_id: request.run_id,
                session: request.session,
                history_messages: request.history_messages,
                tool_context: request.tool_context,
                debug: request.debug,
                input: request.input,
            }));
            apply_agent_executable_socket_env(
                &mut command,
                runtime,
                request,
            );
            command.stdout(Stdio::piped()).process_group(0);
            command
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BwrapAgentExecutableArgs<'a> {
    pub runtime: AgentExecutableSocketRuntime<'a>,
    pub mount_table: &'a MountTable,
    pub cwd: &'a str,
    pub workspace: Option<&'a str>,
    pub run_id: &'a str,
    pub session: &'a str,
    pub history_messages: &'a str,
    pub tool_context: &'a str,
    pub debug: Option<SocketDebugTiming>,
    pub input: &'a str,
}

fn apply_agent_executable_socket_env(
    command: &mut Command,
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
) {
    command
        .env_clear()
        .envs(
            runtime
                .env
                .iter()
                .filter(|env| !is_provider_secret_env(&env.0))
                .map(|env| (env.0.as_str(), env.1.as_str())),
        )
        .env("CTX_AGENT", runtime.agent_name)
        .env("CTX_ROOT", runtime.ctx_root)
        .env("CTX_SOURCE", runtime.source_root)
        .env("CTX_RUN_ID", request.run_id)
        .env("CTX_SESSION", request.session)
        .env("CTX_AGENT_HISTORY_MESSAGES", request.history_messages)
        .env("CTX_AGENT_TOOL_CONTEXT", request.tool_context);
    if let Some(workspace) = request.workspace {
        command.env("CTX_WORKSPACE", workspace);
    }
}

pub(crate) fn agent_executable_socket_bwrap_args(
    request: &BwrapAgentExecutableArgs<'_>,
) -> Vec<String> {
    let mut bwrap = vec!["--clearenv".to_owned()];
    for env in request.runtime.env {
        if env.0 == "CTX_PROVIDER_CONFIG_DIR" || is_provider_secret_env(&env.0) {
            continue;
        }
        bwrap.extend(["--setenv".to_owned(), env.0.clone(), env.1.clone()]);
    }
    bwrap.extend([
        "--setenv".to_owned(),
        "CTX_AGENT".to_owned(),
        request.runtime.agent_name.to_owned(),
        "--setenv".to_owned(),
        "CTX_ROOT".to_owned(),
        request.runtime.ctx_root.display().to_string(),
        "--setenv".to_owned(),
        "CTX_PROVIDER_CONFIG_DIR".to_owned(),
        request
            .runtime
            .ctx_root
            .join("shared/providers.d")
            .display()
            .to_string(),
        "--setenv".to_owned(),
        "CTX_SOURCE".to_owned(),
        request.runtime.source_root.display().to_string(),
        "--setenv".to_owned(),
        "CTX_RUN_ID".to_owned(),
        request.run_id.to_owned(),
        "--setenv".to_owned(),
        "CTX_SESSION".to_owned(),
        request.session.to_owned(),
        "--setenv".to_owned(),
        "CTX_AGENT_HISTORY_MESSAGES".to_owned(),
        request.history_messages.to_owned(),
        "--setenv".to_owned(),
        "CTX_AGENT_TOOL_CONTEXT".to_owned(),
        request.tool_context.to_owned(),
        "--setenv".to_owned(),
        "CTX_WORKSPACE".to_owned(),
        request.workspace.unwrap_or("").to_owned(),
        "--die-with-parent".to_owned(),
        "--unshare-pid".to_owned(),
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--dir".to_owned(),
        "/run".to_owned(),
        "--dir".to_owned(),
        "/home".to_owned(),
        "--ro-bind".to_owned(),
        "/usr".to_owned(),
        "/usr".to_owned(),
        "--ro-bind".to_owned(),
        "/etc".to_owned(),
        "/etc".to_owned(),
        "--tmpfs".to_owned(),
        "/etc/profile.d".to_owned(),
        "--symlink".to_owned(),
        "usr/bin".to_owned(),
        "/bin".to_owned(),
        "--symlink".to_owned(),
        "usr/lib".to_owned(),
        "/lib".to_owned(),
        "--symlink".to_owned(),
        "usr/lib".to_owned(),
        "/lib64".to_owned(),
    ]);
    bwrap.push("--unshare-net".to_owned());
    bwrap.extend(bwrap_source_root_bind_args(request.runtime.source_root));
    if let Some(timing) = request.debug {
        bwrap.extend([
            "--setenv".to_owned(),
            "CTX_AGENT_DEBUG_TIMING".to_owned(),
            "1".to_owned(),
            "--setenv".to_owned(),
            "CTX_AGENT_DEBUG_START_UNIX_MS".to_owned(),
            timing.start_unix_ms.to_string(),
        ]);
    }
    for mount in request.mount_table.entries() {
        bwrap.push(match mount.mode() {
            MountMode::ReadOnly => "--ro-bind".to_owned(),
            MountMode::ReadWrite => "--bind".to_owned(),
        });
        bwrap.push(socket_runtime_host_mount_source(
            request.runtime.source_root,
            mount.source(),
        ));
        bwrap.push(mount.target().to_owned());
    }
    bwrap.extend(bwrap_workspace_bind_args(
        request.cwd,
        request.workspace,
        request.mount_table,
    ));
    bwrap.extend(bwrap_dir_args_for_chdir(request.cwd));
    bwrap.extend([
        "--chdir".to_owned(),
        request.cwd.to_owned(),
        request.runtime.agent_executable.display().to_string(),
        request.input.to_owned(),
    ]);
    bwrap
}

fn is_provider_secret_env(name: &str) -> bool {
    name.starts_with("CTX_PROVIDER_SECRET_")
}

fn bwrap_workspace_bind_args(
    cwd: &str,
    workspace: Option<&str>,
    mount_table: &MountTable,
) -> Vec<String> {
    let Some(workspace) = workspace else {
        return Vec::new();
    };
    if !cwd_uses_default_workspace(cwd) || mount_table_targets_workspace(mount_table) {
        return Vec::new();
    }
    if !is_absolute_host_workspace_path(workspace) {
        return Vec::new();
    }
    vec![
        "--bind".to_owned(),
        workspace.to_owned(),
        "/workspace".to_owned(),
    ]
}

fn cwd_uses_default_workspace(cwd: &str) -> bool {
    cwd == "/workspace" || cwd.starts_with("/workspace/")
}

fn mount_table_targets_workspace(mount_table: &MountTable) -> bool {
    mount_table
        .entries()
        .iter()
        .any(|mount| cwd_uses_default_workspace(mount.target()))
}

fn is_absolute_host_workspace_path(value: &str) -> bool {
    !value.bytes().any(|byte| byte.is_ascii_control())
        && Path::new(value).is_absolute()
        && Path::new(value).components().all(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}

fn socket_runtime_host_mount_source(source_root: &Path, source: &str) -> String {
    let source_path = Path::new(source);
    if source_path == Path::new(CTX_ROOT) {
        return source_root.display().to_string();
    }
    if let Ok(relative) = source_path.strip_prefix(CTX_ROOT) {
        return source_root.join(relative).display().to_string();
    }
    source.to_owned()
}

fn bwrap_source_root_bind_args(source_root: &Path) -> Vec<String> {
    let Some(source_root) = source_root.to_str() else {
        return Vec::new();
    };
    if !source_root.starts_with('/') || source_root == "/" {
        return Vec::new();
    }
    let mut args = bwrap_dir_args_for_parent(source_root);
    args.push("--ro-bind".to_owned());
    args.push(source_root.to_owned());
    args.push(source_root.to_owned());
    args
}

fn bwrap_dir_args_for_parent(path: &str) -> Vec<String> {
    let Some((parent, _name)) = path.rsplit_once('/') else {
        return Vec::new();
    };
    if parent.is_empty() {
        Vec::new()
    } else {
        bwrap_dir_args_for_chdir(parent)
    }
}

fn bwrap_dir_args_for_chdir(cwd: &str) -> Vec<String> {
    let mut args = Vec::new();
    if !cwd.starts_with('/') {
        return args;
    }
    let mut path = String::new();
    for component in cwd.split('/').filter(|component| !component.is_empty()) {
        path.push('/');
        path.push_str(component);
        args.push("--dir".to_owned());
        args.push(path.clone());
    }
    args
}
