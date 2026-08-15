use super::*;
use cortexfs_runtime_client::agent::{AGENT_ENVELOPE_ARG, AGENT_LAUNCH_ABI};
use nix::fcntl::{FcntlArg, fcntl};
use std::os::fd::RawFd;

const SOCKET_AGENT_EXECUTABLE_PATH: &str = "/run/cortexfs/agent-executable";
/// Fixed sandbox path for the receipt-bound per-run control socket.
pub const SOCKET_RUN_CONTROL_PATH: &str = "/run/cortexfs/control.sock";
pub(crate) type RunControlCommand<'a> = (&'a Path, &'a [(String, String)], RawFd);

pub(crate) fn agent_executable_socket_command(
    runtime: AgentExecutableSocketRuntime<'_>,
    agent_executable: &fs::File,
    request: AgentExecutableRunRequest<'_>,
    step: u8,
    control: Option<RunControlCommand<'_>>,
    provider_egress: Option<&Path>,
) -> Result<(Command, Option<Vec<InheritedFd>>), SocketRuntimeError> {
    let environment = agent_executable_socket_env(runtime, request, step);
    let (control_socket, control_environment, control_gate) = match control {
        Some((socket, environment, gate)) => (Some(socket), Some(environment), Some(gate)),
        None => (None, None, None),
    };
    match runtime.execution {
        AgentExecutableSocketExecution::Direct => {
            let mut command = command_for_agent_identity(
                support::plain::proc_fd_path(agent_executable),
                runtime.identity,
            );
            apply_agent_executable_socket_env(&mut command, &environment);
            if let Some(environment) = control_environment {
                command.envs(environment.iter().map(|entry| (&entry.0, &entry.1)));
            }
            command.arg(AGENT_ENVELOPE_ARG).stdin(Stdio::piped());
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
            let mut command = command_for_agent_identity(program, runtime.identity);
            command.args(agent_executable_socket_bwrap_args(
                &BwrapAgentExecutableArgs {
                    runtime,
                    mount_table,
                    cwd: request.cwd.unwrap_or(runtime.default_cwd),
                    debug: request.debug,
                    input: AGENT_ENVELOPE_ARG,
                    agent_executable_fd: agent_executable_fd.raw(),
                    agent_home_source_fd: agent_home_source_fd.raw(),
                    agent_home_sandbox_fd: agent_home_sandbox_fd.raw(),
                    agent_home,
                    environment: &environment,
                    control_socket,
                    control_environment,
                    control_gate,
                    provider_egress,
                },
            ));
            apply_agent_executable_socket_env(&mut command, &environment);
            if let Some(environment) = control_environment {
                command.envs(environment.iter().map(|entry| (&entry.0, &entry.1)));
            }
            command.stdout(Stdio::piped()).process_group(0);
            command.stdin(Stdio::piped());
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
    pub environment: &'a [(String, String)],
    pub control_socket: Option<&'a Path>,
    pub control_environment: Option<&'a [(String, String)]>,
    pub control_gate: Option<RawFd>,
    pub provider_egress: Option<&'a Path>,
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

fn agent_executable_socket_env(
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
    step: u8,
) -> Vec<(String, String)> {
    let mut environment = runtime
        .env
        .iter()
        .filter(|env| !env.0.starts_with("CTX_PROVIDER_SECRET_"))
        .cloned()
        .collect::<Vec<_>>();
    environment.extend([
        ("CTX_AGENT".to_owned(), runtime.agent_name.to_owned()),
        (
            "CTX_ROOT".to_owned(),
            runtime.ctx_root.display().to_string(),
        ),
        (
            "CTX_SOURCE".to_owned(),
            runtime.source_root.display().to_string(),
        ),
        ("CTX_RUN_ID".to_owned(), request.run_id.to_owned()),
        ("CTX_SESSION".to_owned(), request.session.to_owned()),
        ("CTX_AGENT_LAUNCH".to_owned(), AGENT_LAUNCH_ABI.to_owned()),
        ("CTX_AGENT_STEP".to_owned(), step.to_string()),
    ]);
    environment
}

pub(crate) fn apply_agent_executable_socket_env(
    command: &mut Command,
    environment: &[(String, String)],
) {
    command
        .env_clear()
        .envs(environment.iter().map(|entry| (&entry.0, &entry.1)));
}

pub(crate) fn agent_executable_socket_bwrap_args(
    request: &BwrapAgentExecutableArgs<'_>,
) -> Vec<String> {
    let mut bwrap = vec![
        "--clearenv".to_owned(),
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
    ];
    bwrap.extend(support::process::BWRAP_PROCESS_SETUP_ARGS.map(str::to_owned));
    bwrap.extend([
        "--dir".to_owned(),
        "/run/cortexfs".to_owned(),
        "--perms".to_owned(),
        "0755".to_owned(),
        "--ro-bind-data".to_owned(),
        request.agent_executable_fd.to_string(),
        SOCKET_AGENT_EXECUTABLE_PATH.to_owned(),
    ]);
    bwrap.extend(support::process::bwrap_system_layout_args());
    if !request.runtime.network_allowed {
        bwrap.push("--unshare-net".to_owned());
    }
    if let Some(host_dir) = request.provider_egress {
        bwrap.extend([
            "--dir".to_owned(),
            runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH.to_owned(),
            "--ro-bind".to_owned(),
            host_dir.display().to_string(),
            runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH.to_owned(),
            "--setenv".to_owned(),
            runtime::egress::PROVIDER_EGRESS_DIR_ENV.to_owned(),
            runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH.to_owned(),
        ]);
    }
    append_bwrap_agent_environment(&mut bwrap, request.environment, request.control_environment);
    if let Some(socket) = request.control_socket {
        bwrap.extend([
            "--bind".to_owned(),
            socket.display().to_string(),
            SOCKET_RUN_CONTROL_PATH.to_owned(),
        ]);
    }
    if let Some(gate) = request.control_gate {
        bwrap.extend(["--block-fd".to_owned(), gate.to_string()]);
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
    bwrap.extend(support::bwrap::dir_args_for_chdir(request.cwd));
    bwrap.extend([
        "--chdir".to_owned(),
        request.cwd.to_owned(),
        SOCKET_AGENT_EXECUTABLE_PATH.to_owned(),
        request.input.to_owned(),
    ]);
    bwrap
}

fn append_bwrap_agent_environment(
    bwrap: &mut Vec<String>,
    environment: &[(String, String)],
    control_environment: Option<&[(String, String)]>,
) {
    for entry in environment {
        let (name, value) = (&entry.0, &entry.1);
        if matches!(
            name.as_str(),
            "CTX_PROVIDER_CONFIG_DIR" | runtime::egress::PROVIDER_EGRESS_DIR_ENV
        ) {
            continue;
        }
        bwrap.extend(["--setenv".to_owned(), name.clone(), value.clone()]);
    }
    for entry in control_environment.unwrap_or_default() {
        let (name, value) = (&entry.0, &entry.1);
        if name == "CTX_CONTROL_SOCKET" {
            bwrap.extend(["--setenv".to_owned(), name.clone(), value.clone()]);
        }
    }
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
