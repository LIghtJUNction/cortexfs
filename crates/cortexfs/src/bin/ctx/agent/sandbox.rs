use std::path::Component::{Normal, ParentDir};

use crate::*;

pub(crate) fn agent_start_systemd_command(
    root: &Path,
    args: &AgentStartArgs,
    cli_mounts: &[AgentMount],
    view: &AgentRuntimeView,
    socket: &Path,
    unit: &str,
) -> AgentLaunchCommand {
    let request = AgentLaunchRequest {
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
    };
    let mut command = terminal_command(&request, view, socket, unit);
    if !args.command.is_empty() {
        command
            .args
            .retain(|arg| !arg.starts_with("--property=Restart"));
        let _ = command.args.pop();
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
    cortexfs::chat_socket_command(
        &AgentLaunchRequest {
            agent: name.to_owned(),
            session: String::new(),
            source: agent_source_root(root),
            cwd: String::new(),
            mounts: Vec::new(),
            default_workspace: false,
        },
        socket,
        unit,
        Path::new(cortexfs::support::command::CORTEXFS_AGENT_RUNTIME),
    )
}

pub(crate) fn agent_source_root(root: &Path) -> PathBuf {
    read_xattr_string(root, "user.cortexfs.abi_path")
        .filter(String::is_empty)
        .and_then(|_| read_xattr_string(root, "user.cortexfs.backing_path"))
        .map(PathBuf::from)
        .filter(|backing| backing.is_absolute() && open_plain_directory(backing).is_ok())
        .unwrap_or_else(|| root.to_path_buf())
}

pub(crate) fn agent_lifecycle_name(lifecycle: cortexfs::ChildLifecycle) -> &'static str {
    match lifecycle {
        cortexfs::ChildLifecycle::Owned => "owned",
        cortexfs::ChildLifecycle::Temp => "temp",
    }
}

pub(crate) fn agent_start_mounts_with_default_source(
    args: &AgentStartArgs,
    default_source: &Path,
) -> Vec<AgentMount> {
    let mut mounts = Vec::with_capacity(args.mounts.len() + 1);
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

pub(crate) fn agent_start_sandbox_cwd(args: &AgentStartArgs, mounts: &[AgentMount]) -> String {
    for mount in mounts {
        if let Ok(relative) = Path::new(&args.cwd).strip_prefix(&mount.source) {
            return Path::new(&mount.target)
                .join(relative)
                .display()
                .to_string();
        }
    }
    args.cwd.clone()
}

pub(crate) fn agent_start_workspace_source(mounts: &[AgentMount]) -> Option<String> {
    mounts
        .iter()
        .rev()
        .find(|mount| mount.target == "/workspace" && mount.mode == "rw")
        .map(|mount| mount.source.clone())
}

pub(crate) fn validate_agent_start_mounts(
    view: &AgentRuntimeView,
    mounts: &[AgentMount],
) -> Result<(), CliError> {
    let policy = view.mount_table().entries();
    mounts
        .iter()
        .all(|mount| {
            policy.iter().any(|entry| {
                entry.source() == mount.source
                    && entry.target() == mount.target
                    && (entry.mode() == cortexfs::MountMode::ReadWrite || mount.mode == "ro")
            })
        })
        .then_some(())
        .ok_or_else(|| CliError::usage("mount exceeds agent mount policy"))
}

pub(crate) fn require_agent_mount(mount: &AgentMount) -> Result<(), CliError> {
    for (value, label) in [(&mount.source, "source"), (&mount.target, "target")] {
        if value.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(CliError::usage(format!(
                "agent mount {label} must not contain control characters"
            )));
        }
        if !Path::new(value).is_absolute() {
            return Err(CliError::usage(format!(
                "agent mount {label} must be absolute"
            )));
        }
    }
    if !matches!(mount.mode.as_str(), "ro" | "rw") {
        return Err(CliError::usage("agent mount mode must be ro or rw"));
    }
    if is_protected_agent_mount_target(&mount.target) {
        return Err(CliError::usage(
            "agent mount target cannot replace sandbox system paths",
        ));
    }
    Ok(())
}

pub(crate) fn is_protected_agent_mount_target(target: &str) -> bool {
    let mut normalized = PathBuf::from("/");
    for component in Path::new(target).components() {
        if component == ParentDir {
            normalized.pop();
        } else if let Normal(part) = component {
            normalized.push(part);
        }
    }
    let top = normalized.components().nth(1).and_then(|component| component.as_os_str().to_str());
    top.is_none_or(|top| {
        [
            "bin", "ctx", "dev", "etc", "home", "lib", "lib64", "proc", "run", "usr",
        ]
        .contains(&top)
    })
}

pub(crate) fn require_sandbox_cwd(cwd: &str) -> Result<(), CliError> {
    Path::new(cwd)
        .is_absolute()
        .then_some(())
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
    for target in [format!("{unit}.service"), format!("{unit}.socket")] {
        for verb in ["stop", "reset-failed"] {
            let _ignored = systemctl_user_command([verb, target.as_str()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

pub(crate) fn stable_path_hash(path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    let path = absolute_existing_path(path).unwrap_or_else(|_error| path.to_path_buf());
    path.display().to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
