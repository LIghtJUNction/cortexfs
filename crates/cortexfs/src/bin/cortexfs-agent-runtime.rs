#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cortexfs::{
    AgentExecutableSocketExecution, AgentExecutableSocketRuntime, PolicyObjectClass,
    PolicyPermission, SocketPeerPolicy, derive_agent_runtime_view,
    read_provider_system_secret_for_model, serve_agent_executable_socket_listener_once,
};
use listenfd::ListenFd;
use nix::fcntl::{AtFlags, OFlag, open, openat};
use nix::sys::stat::{Mode, fchmod, fstatat};
use nix::unistd::fchown;
use nix::unistd::{Gid, Uid};

const DEFAULT_SOURCE: &str = "/var/lib/cortexfs/storage/v1-root";

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = write_error(&format!("cortexfs-agent-runtime: {error}"));
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), String> {
    let config = RuntimeConfig::parse(args)?;
    let view = derive_agent_runtime_view(&config.source, &config.agent)
        .map_err(|error| format!("agent view {}: {}", error.errno(), config.agent))?;
    let mut listenfd = ListenFd::from_env();
    let Some(listener) = listenfd
        .take_unix_listener(0)
        .map_err(|error| format!("invalid systemd Unix listener: {error}"))?
    else {
        return Err(
            "missing systemd Unix listener fd; start via cortexfs-agent@.socket".to_owned(),
        );
    };

    let session_root = view.home().join("session");
    let default_cwd = view.cwd().display().to_string();
    let peer_policy = SocketPeerPolicy::uid(view.identity().uid());
    repair_agent_session_permissions(&session_root, view.identity().uid(), view.identity().gid())?;
    let runtime_model = runtime_model(&config.source, view.model());
    let network_allowed = view.policy().allows(
        view.policy_subject(),
        PolicyObjectClass::Network,
        "default",
        PolicyPermission::Connect,
    );
    let mut runtime_env = view.env().to_vec();
    if runtime_model != view.model() {
        runtime_env.push(("CTX_AGENT_MODEL_OVERRIDE".to_owned(), runtime_model.clone()));
    }
    let provider_secret =
        read_provider_system_secret_for_model(Path::new(cortexfs::CTX_ROOT), &runtime_model)
            .map_err(|_error| format!("provider secret unavailable for model: {runtime_model}"))?;
    if let Some(secret) = provider_secret.as_ref() {
        runtime_env.extend([
            (
                "CTX_PROVIDER_SECRET_VALUE".to_owned(),
                secret.secret().to_owned(),
            ),
            (
                "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
                secret.provider().to_owned(),
            ),
            (
                "CTX_PROVIDER_SECRET_SLOT".to_owned(),
                secret.account().to_owned(),
            ),
        ]);
    }
    let agent_executable = runtime_agent_executable(Path::new(cortexfs::CTX_ROOT), &config.agent);
    let result = serve_agent_executable_socket_listener_once(
        &listener,
        Some(peer_policy),
        AgentExecutableSocketRuntime {
            ctx_root: Path::new(cortexfs::CTX_ROOT),
            source_root: &config.source,
            identity: view.identity(),
            env: &runtime_env,
            session_root: &session_root,
            default_cwd: &default_cwd,
            model: Some(&runtime_model),
            network_allowed,
            agent_name: view.agent_name(),
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    repair_agent_session_permissions(&session_root, view.identity().uid(), view.identity().gid())?;
    result.map(|_response| ()).map_err(|error| {
        format!(
            "socket runtime {} {error:?}: {}",
            error.errno(),
            config.agent
        )
    })
}

fn runtime_model(_source: &Path, requested_model: &str) -> String {
    requested_model.to_owned()
}

fn runtime_agent_executable(ctx_root: &Path, agent: &str) -> PathBuf {
    ctx_root.join("agent").join(agent)
}

fn repair_agent_session_permissions(session_root: &Path, uid: u32, gid: u32) -> Result<(), String> {
    match fs::symlink_metadata(session_root) {
        Ok(_metadata) => repair_path_permissions(session_root, uid, gid),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "cannot inspect session path {}: {error}",
            session_root.display()
        )),
    }
}

fn repair_path_permissions(path: &Path, uid: u32, gid: u32) -> Result<(), String> {
    if path
        .file_name()
        .is_some_and(|name| name == "workspace-overlay")
    {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect session path {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if !metadata.is_dir() && !metadata.is_file() {
        return Ok(());
    }
    let fd = open_session_repair_path_no_follow(path, metadata.is_dir())?;
    repair_open_path_permissions(
        &fd,
        &path.display().to_string(),
        metadata.is_dir(),
        uid,
        gid,
    )
}

fn repair_open_path_permissions(
    fd: &OwnedFd,
    label: &str,
    is_dir: bool,
    uid: u32,
    gid: u32,
) -> Result<(), String> {
    if let Err(error) = fchown(fd, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid))) {
        if is_read_only_permission_repair_error(error) {
            return Ok(());
        }
        return Err(format!("cannot chown session path {label}: {error}"));
    }
    let mode = if is_dir { 0o700 } else { 0o600 };
    if let Err(error) = fchmod(fd, Mode::from_bits_truncate(mode)) {
        if is_read_only_permission_repair_error(error) {
            return Ok(());
        }
        return Err(format!("cannot chmod session path {label}: {error}"));
    }
    if is_dir {
        for entry in fs::read_dir(proc_fd_path(fd))
            .map_err(|error| format!("cannot read session dir {label}: {error}"))?
        {
            let entry = entry.map_err(|error| format!("cannot read session dir entry: {error}"))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_error| "session path contains invalid component".to_owned())?;
            repair_child_path_permissions(fd, label, &name, uid, gid)?;
        }
    }
    Ok(())
}

fn is_read_only_permission_repair_error(error: nix::errno::Errno) -> bool {
    error == nix::errno::Errno::EROFS
}

fn repair_child_path_permissions(
    parent_fd: &OwnedFd,
    parent_label: &str,
    name: &str,
    uid: u32,
    gid: u32,
) -> Result<(), String> {
    let stat = fstatat(parent_fd, name, AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|error| format!("cannot inspect session path {parent_label}/{name}: {error}"))?;
    let file_type = stat.st_mode & nix::libc::S_IFMT;
    if file_type == nix::libc::S_IFLNK {
        return Ok(());
    }
    let is_dir = file_type == nix::libc::S_IFDIR;
    if !is_dir && file_type != nix::libc::S_IFREG {
        return Ok(());
    }
    let mut flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    if is_dir {
        flags |= OFlag::O_DIRECTORY;
    }
    let fd = openat(parent_fd, name, flags, Mode::empty())
        .map_err(|error| format!("cannot open session path {parent_label}/{name}: {error}"))?;
    repair_open_path_permissions(&fd, &format!("{parent_label}/{name}"), is_dir, uid, gid)
}

fn proc_fd_path(fd: &OwnedFd) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd()))
}

fn open_session_repair_path_no_follow(path: &Path, is_dir: bool) -> Result<OwnedFd, String> {
    let mut current = if path.is_absolute() {
        open_dir_no_follow(Path::new("/"))?
    } else {
        open_dir_no_follow(Path::new("."))?
    };
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name
                    .to_str()
                    .ok_or_else(|| "session path contains invalid component".to_owned())?;
                let final_component = components.peek().is_none();
                let mut flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
                if !final_component || is_dir {
                    flags |= OFlag::O_DIRECTORY;
                }
                current = openat(&current, name, flags, Mode::empty()).map_err(|error| {
                    format!("cannot open session path {}: {error}", path.display())
                })?;
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(format!(
                    "cannot open session path {}: unsupported path component",
                    path.display()
                ));
            }
        }
    }
    Ok(current)
}

fn open_dir_no_follow(path: &Path) -> Result<OwnedFd, String> {
    open(
        path,
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("cannot open runtime credential dir: {error}"))
}

fn write_error(line: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(line.as_bytes())
        .and_then(|()| stderr.write_all(b"\n"))
}

#[derive(Debug, Eq, PartialEq)]
struct RuntimeConfig {
    source: PathBuf,
    agent: String,
}

impl RuntimeConfig {
    fn parse(args: Vec<OsString>) -> Result<Self, String> {
        let mut source = PathBuf::from(DEFAULT_SOURCE);
        let mut agent = None;
        let mut values = args.into_iter();

        while let Some(value) = values.next() {
            if value == "--source" || value == "-s" {
                let Some(next) = values.next() else {
                    return Err("--source requires a path".to_owned());
                };
                source = PathBuf::from(next);
                continue;
            }
            if value == "--agent" || value == "-a" {
                let Some(next) = values.next() else {
                    return Err("--agent requires a name".to_owned());
                };
                agent = Some(os_string(next)?);
                continue;
            }
            if value == "--help" || value == "-h" {
                return Err(usage());
            }
            if agent.is_none() {
                agent = Some(os_string(value)?);
                continue;
            }
            return Err("unexpected extra argument".to_owned());
        }

        let Some(agent) = agent else {
            return Err(usage());
        };
        Ok(Self { source, agent })
    }
}

fn os_string(value: OsString) -> Result<String, String> {
    value
        .into_string()
        .map_err(|value| format!("arguments must be valid UTF-8: {}", value.to_string_lossy()))
}

fn usage() -> String {
    "usage: cortexfs-agent-runtime [--source CTX_SOURCE] --agent AGENT".to_owned()
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/cortexfs_agent_runtime_tests.rs"
    ));
}
