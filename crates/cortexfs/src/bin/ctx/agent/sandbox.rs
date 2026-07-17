use crate::*;

pub(crate) fn agent_start_systemd_command(
    root: &Path,
    args: &AgentStartArgs,
    cli_mounts: &[AgentMount],
    view: &AgentRuntimeView,
    socket: &Path,
    unit: &str,
) -> AgentLaunchCommand {
    let home = view.ctx_home();
    let mut command = AgentLaunchCommand {
        program: SYSTEMD_RUN_PROGRAM.to_owned(),
        args: vec![
            "--user".to_owned(),
            "--unit".to_owned(),
            unit.to_owned(),
            "--property".to_owned(),
            "Restart=always".to_owned(),
            "--property".to_owned(),
            "RestartSec=250ms".to_owned(),
            "/usr/bin/env".to_owned(),
            "-i".to_owned(),
            "PATH=/usr/bin:/bin".to_owned(),
            "/usr/bin/bwrap".to_owned(),
        ],
    };
    command
        .args
        .extend(agent_bwrap_args(root, args, cli_mounts, view, socket, home));
    command
}

#[cfg(test)]
pub(crate) fn agent_chat_socket_systemd_command(
    root: &Path,
    name: &str,
    socket: &Path,
    unit: &str,
) -> AgentLaunchCommand {
    let source = agent_source_root(root);
    cortexfs::chat_socket_command(
        &cortexfs::AgentLaunchRequest {
            agent: name.to_owned(),
            session: String::new(),
            source,
            cwd: String::new(),
            mounts: Vec::new(),
            default_workspace: false,
        },
        socket,
        unit,
        Path::new(&agent_runtime_program()),
    )
}

pub(crate) fn agent_source_root(root: &Path) -> PathBuf {
    if read_xattr_string(root, "user.cortexfs.abi_path").as_deref() != Some("") {
        return root.to_path_buf();
    }
    let Some(backing) = read_xattr_string(root, "user.cortexfs.backing_path").map(PathBuf::from)
    else {
        return root.to_path_buf();
    };
    if backing.is_absolute() && open_plain_directory(&backing).is_ok() {
        backing
    } else {
        root.to_path_buf()
    }
}

#[cfg(test)]
pub(crate) fn agent_runtime_program() -> String {
    if let Ok(current) = env::current_exe()
        && let Some(parent) = current.parent()
    {
        let sibling = parent.join("cortexfs-agent-runtime");
        if sibling.is_file() {
            return sibling.display().to_string();
        }
    }
    "/usr/bin/cortexfs-agent-runtime".to_owned()
}

pub(crate) fn agent_start_process_command(
    identity: &AgentUnixIdentity,
    command: &AgentLaunchCommand,
) -> io::Result<ProcessCommand> {
    launch_process_for(identity, command)
}

pub(crate) fn agent_sandbox_env(_root: &Path, view: &AgentRuntimeView) -> Vec<(String, String)> {
    let sandbox_ctx_home = sandbox_ctx_home(view);
    let groups = view
        .identity()
        .groups()
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let mut env = vec![
        ("CTX_ROOT".to_owned(), CTX_ROOT.to_owned()),
        (
            "CTX_PROVIDER_CONFIG_DIR".to_owned(),
            format!("{CTX_ROOT}/shared/providers.d"),
        ),
        ("CTX_HOME".to_owned(), sandbox_ctx_home),
        ("CTX_AGENT".to_owned(), view.agent_name().to_owned()),
        (
            "CTX_AGENT_ROLE".to_owned(),
            agent_role_for_display(view.agent_name()).to_owned(),
        ),
        ("CTX_AGENT_MODEL".to_owned(), view.model().to_owned()),
        (
            "CTX_AGENT_LIFE".to_owned(),
            agent_lifecycle_name(view.lifecycle()).to_owned(),
        ),
        (
            "CTX_AGENT_ROOT_PATH".to_owned(),
            view.root().display().to_string(),
        ),
        ("CTX_AGENT_CWD".to_owned(), view.cwd().display().to_string()),
        (
            "CTX_AGENT_SUBJECT".to_owned(),
            view.policy_subject().to_owned(),
        ),
        (
            "CTX_AGENT_UID".to_owned(),
            view.identity().uid().to_string(),
        ),
        (
            "CTX_AGENT_GID".to_owned(),
            view.identity().gid().to_string(),
        ),
        ("CTX_AGENT_GROUPS".to_owned(), groups),
        ("HOME".to_owned(), AGENT_SANDBOX_HOME.to_owned()),
        ("USER".to_owned(), view.agent_name().to_owned()),
        ("LOGNAME".to_owned(), view.agent_name().to_owned()),
        ("SHELL".to_owned(), "/usr/bin/bash".to_owned()),
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
        ("GIT_OPTIONAL_LOCKS".to_owned(), "0".to_owned()),
    ];
    for env_pair in view.env() {
        let key = &env_pair.0;
        let value = &env_pair.1;
        if matches!(
            key.as_str(),
            "CTX_ROOT"
                | "CTX_PROVIDER_CONFIG_DIR"
                | "CTX_HOME"
                | "CTX_AGENT"
                | "CTX_AGENT_ROLE"
                | "CTX_AGENT_MODEL"
                | "CTX_AGENT_LIFE"
                | "CTX_AGENT_ROOT_PATH"
                | "CTX_AGENT_CWD"
                | "CTX_AGENT_SUBJECT"
                | "CTX_AGENT_UID"
                | "CTX_AGENT_GID"
                | "CTX_AGENT_GROUPS"
                | "HOME"
                | "USER"
                | "LOGNAME"
                | "SHELL"
                | "TERM"
                | "LANG"
                | "GIT_OPTIONAL_LOCKS"
        ) {
            continue;
        }
        env.push((key.clone(), value.clone()));
    }
    env
}

pub(crate) fn agent_lifecycle_name(lifecycle: cortexfs::ChildLifecycle) -> &'static str {
    match lifecycle {
        cortexfs::ChildLifecycle::Owned => "owned",
        cortexfs::ChildLifecycle::Temp => "temp",
    }
}

pub(crate) fn agent_bwrap_args(
    root: &Path,
    args: &AgentStartArgs,
    cli_mounts: &[AgentMount],
    view: &AgentRuntimeView,
    socket: &Path,
    _home: &Path,
) -> Vec<String> {
    let agent_home = view.home();
    let mut bwrap = vec!["--clearenv".to_owned()];
    for (key, value) in agent_sandbox_env(root, view) {
        bwrap.extend(["--setenv".to_owned(), key, value]);
    }
    if let Some(workspace) = agent_start_workspace_source(cli_mounts) {
        bwrap.extend(["--setenv".to_owned(), "CTX_WORKSPACE".to_owned(), workspace]);
    }
    bwrap.extend([
        "--die-with-parent".to_owned(),
        "--unshare-pid".to_owned(),
        "--unshare-net".to_owned(),
    ]);
    bwrap.extend(cortexfs::support::process::BWRAP_PROCESS_SETUP_ARGS.map(str::to_owned));
    bwrap.extend(cortexfs::support::process::BWRAP_SYSTEM_LAYOUT_ARGS.map(str::to_owned));
    if let Some(runtime_dir) = socket_runtime_dir(socket) {
        bwrap.extend([
            "--bind".to_owned(),
            runtime_dir.display().to_string(),
            runtime_dir.display().to_string(),
        ]);
    }
    let has_policy_git_mount = has_policy_git_mount(view);
    for mount in view.mount_table().entries() {
        if args.default_workspace && mount.target() == "/workspace/.git" {
            continue;
        }
        bwrap.push(match mount.mode() {
            cortexfs::MountMode::ReadOnly => "--ro-bind".to_owned(),
            cortexfs::MountMode::ReadWrite => "--bind".to_owned(),
        });
        bwrap.push(agent_host_mount_source(root, mount.source()));
        let target = if mount.target() == agent_home {
            AGENT_SANDBOX_HOME.to_owned()
        } else {
            mount.target().to_owned()
        };
        bwrap.push(target);
    }
    let git_mask = agent_git_mask(args, cli_mounts, view);
    let mut git_masked = false;
    for mount in cli_mounts {
        bwrap.extend(agent_bwrap_dir_args_for_parent(&mount.target));
        bwrap.push(match mount.mode.as_str() {
            "ro" => "--ro-bind".to_owned(),
            _ => "--bind".to_owned(),
        });
        bwrap.push(mount.source.clone());
        bwrap.push(mount.target.clone());
        if !git_masked && args.default_workspace && mount.target == "/workspace" {
            if has_policy_git_mount {
                for mount in view
                    .mount_table()
                    .entries()
                    .iter()
                    .filter(|mount| mount.target() == "/workspace/.git")
                {
                    bwrap.push(match mount.mode() {
                        cortexfs::MountMode::ReadOnly => "--ro-bind".to_owned(),
                        cortexfs::MountMode::ReadWrite => "--bind".to_owned(),
                    });
                    bwrap.push(agent_host_mount_source(root, mount.source()));
                    bwrap.push(mount.target().to_owned());
                }
            } else if let Some(mask) = git_mask {
                bwrap.extend(agent_git_mask_args(mask));
            }
            git_masked = true;
        }
    }
    if let Some(startup_stub) = shell_startup_stub_path(socket) {
        bwrap.extend([
            "--ro-bind".to_owned(),
            startup_stub.display().to_string(),
            "/etc/profile".to_owned(),
            "--ro-bind".to_owned(),
            startup_stub.display().to_string(),
            "/etc/bash.bashrc".to_owned(),
        ]);
    }
    let sandbox_cwd = agent_start_sandbox_cwd(args, cli_mounts);
    bwrap.extend([
        "--chdir".to_owned(),
        sandbox_cwd,
        "/usr/bin/ctxterm".to_owned(),
        "--listen".to_owned(),
        socket.display().to_string(),
        "--no-stdio".to_owned(),
        "--".to_owned(),
        "/ctx/bin/tsh".to_owned(),
    ]);
    bwrap
}

pub(crate) fn sandbox_ctx_home(view: &AgentRuntimeView) -> String {
    let owner = view
        .ctx_home()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("1000");
    Path::new(CTX_ROOT)
        .join("home")
        .join(owner)
        .display()
        .to_string()
}

pub(crate) fn agent_host_mount_source(root: &Path, source: &str) -> String {
    let source = Path::new(source);
    if source == Path::new(CTX_ROOT) {
        return root.display().to_string();
    }
    if let Ok(relative) = source.strip_prefix(CTX_ROOT) {
        return root.join(relative).display().to_string();
    }
    source.display().to_string()
}

pub(crate) fn agent_start_sandbox_cwd(args: &AgentStartArgs, mounts: &[AgentMount]) -> String {
    let cwd = Path::new(&args.cwd);
    for mount in mounts {
        let source = Path::new(&mount.source);
        let Ok(relative) = cwd.strip_prefix(source) else {
            continue;
        };
        let mut target = PathBuf::from(&mount.target);
        if !relative.as_os_str().is_empty() {
            target.push(relative);
        }
        return target.display().to_string();
    }
    args.cwd.clone()
}

pub(crate) fn agent_start_mounts(args: &AgentStartArgs) -> Result<Vec<AgentMount>, CliError> {
    let default_source = env::current_dir().map_err(|error| {
        CliError::unavailable(format!("cannot read current directory: {error}"))
    })?;
    Ok(agent_start_mounts_with_default_source(
        args,
        &default_source,
    ))
}

pub(crate) fn agent_start_mounts_with_default_source(
    args: &AgentStartArgs,
    default_source: &Path,
) -> Vec<AgentMount> {
    let mut mounts = Vec::new();
    if args.default_workspace {
        mounts.push(AgentMount {
            source: default_source.display().to_string(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        });
    }
    mounts.extend(args.mounts.iter().cloned());
    mounts
}

const SYSTEMD_RUN_PROGRAM: &str = "/usr/bin/systemd-run";

#[derive(Clone, Copy)]
enum AgentGitMask {
    Directory,
    File,
}

fn has_policy_git_mount(view: &AgentRuntimeView) -> bool {
    view.mount_table()
        .entries()
        .iter()
        .any(|mount| mount.target() == "/workspace/.git")
}

fn agent_git_mask(
    args: &AgentStartArgs,
    mounts: &[AgentMount],
    view: &AgentRuntimeView,
) -> Option<AgentGitMask> {
    if !args.default_workspace
        || mounts.iter().any(|mount| mount.target == "/workspace/.git")
        || has_policy_git_mount(view)
    {
        return None;
    }
    let workspace = mounts
        .iter()
        .find(|mount| mount.target == "/workspace" && mount.mode == "rw")?;
    let metadata = fs::symlink_metadata(Path::new(&workspace.source).join(".git")).ok()?;
    if metadata.is_dir() {
        Some(AgentGitMask::Directory)
    } else if metadata.is_file() {
        Some(AgentGitMask::File)
    } else {
        None
    }
}

fn agent_git_mask_args(mask: AgentGitMask) -> Vec<String> {
    match mask {
        AgentGitMask::Directory => vec!["--tmpfs".to_owned(), "/workspace/.git".to_owned()],
        AgentGitMask::File => vec![
            "--ro-bind".to_owned(),
            "/dev/null".to_owned(),
            "/workspace/.git".to_owned(),
        ],
    }
}

pub(crate) fn normalized_absolute_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::Prefix(_prefix) => return None,
        }
    }
    Some(normalized)
}

pub(crate) fn agent_bwrap_dir_args_for_parent(path: &str) -> Vec<String> {
    let Some((parent, _name)) = path.rsplit_once('/') else {
        return Vec::new();
    };
    if parent.is_empty() {
        Vec::new()
    } else {
        agent_bwrap_dir_args_for_path(parent)
    }
}

pub(crate) fn agent_bwrap_dir_args_for_path(path: &str) -> Vec<String> {
    let mut args = Vec::new();
    if !path.starts_with('/') {
        return args;
    }
    let mut current = String::new();
    for component in path.split('/').filter(|component| !component.is_empty()) {
        current.push('/');
        current.push_str(component);
        args.push("--dir".to_owned());
        args.push(current.clone());
    }
    args
}

pub(crate) fn agent_start_workspace_source(mounts: &[AgentMount]) -> Option<String> {
    mounts
        .iter()
        .rev()
        .find(|mount| mount.target == "/workspace" && mount.mode == "rw")
        .map(|mount| mount.source.clone())
}

pub(crate) fn require_agent_mount(mount: &AgentMount) -> Result<(), CliError> {
    if mount.source.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(CliError::usage(
            "agent mount source must not contain control characters",
        ));
    }
    if !Path::new(&mount.source).is_absolute() {
        return Err(CliError::usage("agent mount source must be absolute"));
    }
    if mount.target.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(CliError::usage(
            "agent mount target must not contain control characters",
        ));
    }
    let Some(target) =
        normalized_absolute_path(Path::new(&mount.target)).map(|path| path.display().to_string())
    else {
        return Err(CliError::usage("agent mount target must be absolute"));
    };
    if !matches!(mount.mode.as_str(), "ro" | "rw") {
        return Err(CliError::usage("agent mount mode must be ro or rw"));
    }
    if is_protected_agent_mount_target(&target) {
        return Err(CliError::usage(
            "agent mount target cannot replace sandbox system paths",
        ));
    }
    Ok(())
}

pub(crate) fn is_protected_agent_mount_target(target: &str) -> bool {
    const PROTECTED_TARGETS: &[&str] = &[
        "/", "/bin", "/ctx", "/dev", "/etc", "/home", "/lib", "/lib64", "/proc", "/run", "/usr",
    ];

    PROTECTED_TARGETS
        .iter()
        .any(|protected| target == *protected || target.starts_with(&format!("{protected}/")))
}

pub(crate) fn require_sandbox_cwd(cwd: &str) -> Result<(), CliError> {
    if Path::new(cwd).is_absolute() {
        Ok(())
    } else {
        Err(CliError::usage(
            "agent cwd must be absolute inside the sandbox",
        ))
    }
}

pub(crate) fn ensure_agent_terminal_socket(
    visible_socket: &Path,
    runtime_socket: &Path,
) -> Result<bool, CliError> {
    if let Some(parent) = runtime_socket.parent() {
        create_agent_terminal_runtime_dir(parent).map_err(|error| {
            CliError::unavailable(format!("cannot create {}: {error}", parent.display()))
        })?;
        write_empty_shell_startup_stub(parent).map_err(|error| {
            CliError::unavailable(format!(
                "cannot create {}: {error}",
                parent.join(".empty-shell-startup").display()
            ))
        })?;
    }
    match remove_stale_socket(runtime_socket) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot remove stale {}: {error}",
                runtime_socket.display()
            )));
        }
    }
    ensure_best_effort_visible_terminal_socket(visible_socket, runtime_socket)
}

#[cfg(test)]
pub(crate) fn agent_chat_unit(root: &Path, name: &str) -> String {
    format!("cortexfs-agent-{name}-{}-chat", stable_path_hash(root))
}

pub(crate) fn agent_chat_runtime_socket(root: &Path, name: &str) -> Result<PathBuf, CliError> {
    require_cli_name("agent name", name)?;
    let runtime_root = match env::var_os("XDG_RUNTIME_DIR") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from("/run")
            .join("user")
            .join(current_uid_for_ctx(root)?),
    };
    Ok(runtime_root
        .join("cortexfs")
        .join("agent")
        .join(stable_path_hash(root))
        .join(format!("{name}.sock")))
}

pub(crate) fn reset_agent_chat_unit(unit: &str) {
    let service = format!("{unit}.service");
    let socket = format!("{unit}.socket");
    for target in [service.as_str(), socket.as_str()] {
        let _ignored = systemctl_user_command(["stop", target])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ignored = systemctl_user_command(["reset-failed", target])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub(crate) fn stable_path_hash(path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    absolute_existing_path(path)
        .unwrap_or_else(|_error| path.to_path_buf())
        .display()
        .to_string()
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
