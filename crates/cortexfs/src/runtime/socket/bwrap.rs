use super::*;
use nix::fcntl::{FcntlArg, fcntl};
use std::os::fd::RawFd;

const SOCKET_AGENT_EXECUTABLE_PATH: &str = "/run/cortexfs/agent-executable";
/// Fixed sandbox path for the receipt-bound per-run control socket.
pub const SOCKET_RUN_CONTROL_PATH: &str = "/run/cortexfs/control.sock";
pub(crate) type RunControlCommand<'a> = (&'a Path, &'a [(String, String)]);

pub(crate) fn agent_executable_socket_command(
    runtime: AgentExecutableSocketRuntime<'_>,
    agent_executable: &fs::File,
    request: AgentExecutableRunRequest<'_>,
    control: Option<RunControlCommand<'_>>,
) -> Result<(Command, Option<Vec<InheritedFd>>), SocketRuntimeError> {
    match runtime.execution {
        AgentExecutableSocketExecution::Direct => {
            let mut command = Command::new(support::plain::proc_fd_path(agent_executable));
            apply_agent_executable_socket_env(&mut command, runtime, request);
            if let Some((_socket, environment)) = control {
                command.envs(environment.iter().map(|entry| (&entry.0, &entry.1)));
            }
            command.arg(
                request
                    .envelope
                    .map_or(request.input, |_| "--cortexfs-sdk-envelope-v1"),
            );
            command.stdin(if request.envelope.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
            command.stdout(Stdio::piped()).process_group(0);
            Ok((command, None))
        }
        AgentExecutableSocketExecution::Bwrap {
            program,
            mount_table,
            ..
        } => {
            let agent_executable_fd = InheritedFd::duplicate(agent_executable)?;
            let agent_home = runtime
                .session_root
                .parent()
                .ok_or(SocketRuntimeError::CannotRunAgent)?;
            let agent_home_dir = open_plain_directory(agent_home)
                .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
            let agent_home_source_fd = InheritedFd::duplicate(&agent_home_dir)?;
            let agent_home_sandbox_fd = InheritedFd::duplicate(&agent_home_dir)?;
            let mut command = Command::new(program);
            command.args(agent_executable_socket_bwrap_args(
                &BwrapAgentExecutableArgs {
                    runtime,
                    mount_table,
                    cwd: request.cwd.unwrap_or(runtime.default_cwd),
                    debug: request.debug,
                    input: request
                        .envelope
                        .map_or(request.input, |_| "--cortexfs-sdk-envelope-v1"),
                    agent_executable_fd: agent_executable_fd.raw(),
                    agent_home_source_fd: agent_home_source_fd.raw(),
                    agent_home_sandbox_fd: agent_home_sandbox_fd.raw(),
                    agent_home,
                    control_socket: control.map(|(socket, _environment)| socket),
                },
            ));
            apply_agent_executable_socket_env(&mut command, runtime, request);
            if let Some((_socket, environment)) = control {
                command.envs(environment.iter().map(|entry| (&entry.0, &entry.1)));
            }
            command.stdout(Stdio::piped()).process_group(0);
            command.stdin(if request.envelope.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
            Ok((
                command,
                Some(vec![
                    agent_executable_fd,
                    agent_home_source_fd,
                    agent_home_sandbox_fd,
                ]),
            ))
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BwrapAgentExecutableArgs<'a> {
    pub runtime: AgentExecutableSocketRuntime<'a>,
    pub mount_table: &'a MountTable,
    pub cwd: &'a str,
    pub debug: Option<SocketDebugTiming>,
    pub input: &'a str,
    pub agent_executable_fd: RawFd,
    pub agent_home_source_fd: RawFd,
    pub agent_home_sandbox_fd: RawFd,
    pub agent_home: &'a Path,
    pub control_socket: Option<&'a Path>,
}

pub(crate) struct InheritedFd(RawFd);

impl InheritedFd {
    fn duplicate(fd: &impl std::os::fd::AsFd) -> Result<Self, SocketRuntimeError> {
        fcntl(fd, FcntlArg::F_DUPFD(10))
            .map(Self)
            .map_err(|_error| SocketRuntimeError::CannotRunAgent)
    }

    fn raw(&self) -> RawFd {
        self.0
    }
}

impl Drop for InheritedFd {
    fn drop(&mut self) {
        let _ignored = nix::unistd::close(self.0);
    }
}

pub(crate) fn apply_agent_executable_socket_env(
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
                .filter(|env| {
                    !matches!(runtime.execution, AgentExecutableSocketExecution::Direct)
                        || !env.0.starts_with("CTX_PROVIDER_SECRET_")
                })
                .map(|env| (env.0.as_str(), env.1.as_str())),
        )
        .env("CTX_AGENT", runtime.agent_name)
        .env("CTX_ROOT", runtime.ctx_root)
        .env("CTX_SOURCE", runtime.source_root)
        .env("CTX_RUN_ID", request.run_id)
        .env("CTX_SESSION", request.session);
    if request.envelope.is_some() {
        command
            .env("CTX_AGENT_LAUNCH", "sdk-envelope-v1")
            .env("CTX_AGENT_STEP", request.step.to_string());
    } else {
        command
            .env("CTX_AGENT_HISTORY_MESSAGES", request.history_messages)
            .env("CTX_AGENT_TOOL_CONTEXT", request.tool_context);
    }
}

pub(crate) fn agent_executable_socket_bwrap_args(
    request: &BwrapAgentExecutableArgs<'_>,
) -> Vec<String> {
    let mut bwrap = vec![
        "--setenv".to_owned(),
        "CTX_PROVIDER_CONFIG_DIR".to_owned(),
        request
            .runtime
            .ctx_root
            .join("shared/providers.d")
            .display()
            .to_string(),
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
        "/run/cortexfs".to_owned(),
        "--perms".to_owned(),
        "0755".to_owned(),
        "--ro-bind-data".to_owned(),
        request.agent_executable_fd.to_string(),
        SOCKET_AGENT_EXECUTABLE_PATH.to_owned(),
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
    ];
    if !request.runtime.network_allowed {
        bwrap.push("--unshare-net".to_owned());
    }
    bwrap.extend(bwrap_source_root_bind_args(request.runtime.source_root));
    if let Some(socket) = request.control_socket {
        bwrap.extend([
            "--bind".to_owned(),
            socket.display().to_string(),
            SOCKET_RUN_CONTROL_PATH.to_owned(),
        ]);
    }
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
    bwrap.extend([
        "--bind-fd".to_owned(),
        request.agent_home_source_fd.to_string(),
        request.agent_home.display().to_string(),
        "--bind-fd".to_owned(),
        request.agent_home_sandbox_fd.to_string(),
        "/home/agent".to_owned(),
    ]);
    bwrap.extend(bwrap_dir_args_for_chdir(request.cwd));
    bwrap.extend([
        "--chdir".to_owned(),
        request.cwd.to_owned(),
        SOCKET_AGENT_EXECUTABLE_PATH.to_owned(),
        request.input.to_owned(),
    ]);
    bwrap
}

pub(crate) fn socket_runtime_host_mount_source(source_root: &Path, source: &str) -> String {
    let source_path = Path::new(source);
    if source_path == Path::new(CTX_ROOT) {
        return source_root.display().to_string();
    }
    if let Ok(relative) = source_path.strip_prefix(CTX_ROOT) {
        return source_root.join(relative).display().to_string();
    }
    source.to_owned()
}

pub(crate) fn bwrap_source_root_bind_args(source_root: &Path) -> Vec<String> {
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

pub(crate) fn bwrap_dir_args_for_parent(path: &str) -> Vec<String> {
    let Some((parent, _name)) = path.rsplit_once('/') else {
        return Vec::new();
    };
    if parent.is_empty() {
        Vec::new()
    } else {
        bwrap_dir_args_for_chdir(parent)
    }
}

pub(crate) fn bwrap_dir_args_for_chdir(cwd: &str) -> Vec<String> {
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
