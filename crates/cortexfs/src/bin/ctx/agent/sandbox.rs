use crate::*;

pub(crate) fn agent_start_systemd_command(
    root: &Path,
    args: &AgentStartArgs,
    cli_mounts: &[AgentMount],
    view: &AgentRuntimeView,
    socket: &Path,
    unit: &str,
) -> AgentLaunchCommand {
    let mut command = terminal_command(
        &AgentLaunchRequest {
            agent: args.name.clone(),
            session: args.session.clone(),
            source: root.to_path_buf(),
            cwd: agent_start_sandbox_cwd(args, cli_mounts),
            mounts: cli_mounts
                .iter()
                .map(|mount| AgentLaunchMount {
                    source: mount.source.clone(),
                    target: mount.target.clone(),
                    mode: mount.mode.clone(),
                })
                .collect(),
            default_workspace: args.default_workspace,
        },
        view,
        socket,
        unit,
    );
    if !args.command.is_empty() {
        command.args.pop();
        command.args.extend_from_slice(&args.command);
    }
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
        &AgentLaunchRequest {
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
    if read_xattr_string(root, "user.cortexfs.abi_path").as_deref() == Some("")
        && let Some(backing) = read_xattr_string(root, "user.cortexfs.backing_path").map(PathBuf::from)
        && backing.is_absolute()
        && open_plain_directory(&backing).is_ok()
    {
        return backing;
    }
    root.to_path_buf()
}

#[cfg(test)]
pub(crate) fn agent_runtime_program() -> String {
    env::current_exe().ok().and_then(|current| current.parent().map(|parent| parent.join("cortexfs-agent-runtime")))
        .filter(|sibling| sibling.is_file())
        .map_or_else(|| cortexfs::support::command::CORTEXFS_AGENT_RUNTIME.to_owned(), |path| path.display().to_string())
}

pub(crate) fn agent_lifecycle_name(lifecycle: cortexfs::ChildLifecycle) -> &'static str {
    match lifecycle {
        cortexfs::ChildLifecycle::Owned => "owned",
        cortexfs::ChildLifecycle::Temp => "temp",
    }
}

pub(crate) fn agent_start_mounts_with_default_source(args: &AgentStartArgs, default_source: &Path) -> Vec<AgentMount> {
    let default = args.default_workspace.then(|| AgentMount {
        source: default_source.display().to_string(),
        target: "/workspace".to_owned(),
        mode: "rw".to_owned(),
    });
    default.into_iter().chain(args.mounts.iter().cloned()).collect()
}

pub(crate) fn agent_start_sandbox_cwd(args: &AgentStartArgs, mounts: &[AgentMount]) -> String {
    mounts.iter().find_map(|mount| {
        Path::new(&args.cwd).strip_prefix(&mount.source).ok()
            .map(|relative| Path::new(&mount.target).join(relative).display().to_string())
    }).unwrap_or_else(|| args.cwd.clone())
}

pub(crate) fn agent_start_workspace_source(mounts: &[AgentMount]) -> Option<String> {
    mounts
        .iter()
        .rev()
        .find(|mount| mount.target == "/workspace" && mount.mode == "rw")
        .map(|mount| mount.source.clone())
}

pub(crate) fn validate_agent_start_mounts(view: &AgentRuntimeView, mounts: &[AgentMount]) -> Result<(), CliError> {
    let authorized = |mount: &AgentMount| view.mount_table().entries().iter().any(|entry| {
        entry.source() == mount.source && entry.target() == mount.target
            && (entry.mode() == cortexfs::MountMode::ReadWrite || mount.mode == "ro")
    });
    mounts.iter().all(authorized).then_some(())
        .ok_or_else(|| CliError::usage("mount exceeds agent mount policy"))
}

pub(crate) fn normalized_absolute_path(path: &Path) -> Option<PathBuf> {
    path.is_absolute().then(|| {
        path.components().fold(PathBuf::from("/"), |mut normalized, component| {
            match component {
                std::path::Component::ParentDir => { normalized.pop(); }
                std::path::Component::Normal(part) => normalized.push(part),
                _ => {}
            }
            normalized
        })
    })
}

pub(crate) fn require_agent_mount(mount: &AgentMount) -> Result<(), CliError> {
    if mount.source.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(CliError::usage("agent mount source must not contain control characters"));
    }
    if !Path::new(&mount.source).is_absolute() {
        return Err(CliError::usage("agent mount source must be absolute"));
    }
    if mount.target.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(CliError::usage("agent mount target must not contain control characters"));
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
        return Err(CliError::usage("agent mount target cannot replace sandbox system paths"));
    }
    Ok(())
}

pub(crate) fn is_protected_agent_mount_target(target: &str) -> bool {
    const PROTECTED_TARGETS: &[&str] = &[
        "/", "/bin", CTX_ROOT, "/dev", "/etc", "/home", "/lib", "/lib64", "/proc", "/run", "/usr",
    ];

    PROTECTED_TARGETS
        .iter()
        .any(|protected| target == *protected || target.starts_with(&format!("{protected}/")))
}

pub(crate) fn require_sandbox_cwd(cwd: &str) -> Result<(), CliError> {
    Path::new(cwd).is_absolute().then_some(())
        .ok_or_else(|| CliError::usage("agent cwd must be absolute inside the sandbox"))
}

#[cfg(test)]
pub(crate) fn agent_chat_unit(root: &Path, name: &str) -> String {
    format!("cortexfs-agent-{name}-{}-chat", stable_path_hash(root))
}

pub(crate) fn agent_chat_runtime_socket(root: &Path, name: &str) -> Result<PathBuf, CliError> {
    require_cli_name("agent name", name)?;
    let runtime_root = match env::var_os("XDG_RUNTIME_DIR") {
        Some(path) => PathBuf::from(path),
        None => cortexfs_paths::system_run_root()
            .join("user")
            .join(current_uid_for_ctx(root)?),
    };
    Ok(cortexfs_paths::user_agent_runtime_socket(
        &runtime_root,
        &stable_path_hash(root),
        name,
    ))
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
