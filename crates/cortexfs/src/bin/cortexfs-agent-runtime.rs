#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cortexfs::{
    AgentExecutableSocketExecution, AgentExecutableSocketRuntime, SocketPeerPolicy,
    derive_agent_runtime_view, is_object_name, serve_agent_executable_socket_listener_once,
};
use listenfd::ListenFd;
use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag, open, openat};
use nix::sys::stat::{Mode, fchmod, fstatat, mkdirat};
use nix::unistd::{Gid, Uid};
use nix::unistd::{UnlinkatFlags, fchown, unlinkat};

const DEFAULT_SOURCE: &str = "/var/lib/cortexfs/storage/v1-root";
const BWRAP_PROGRAM: &str = "/usr/bin/bwrap";

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
    let (runtime_model, provider_secret) =
        runtime_model_and_secret(&config.source, view.model())
            .map_err(|_error| format!("provider secret unavailable: {}", view.model()))?;
    let mut runtime_env = view.env().to_vec();
    if runtime_model != view.model() {
        runtime_env.push(("CTX_AGENT_MODEL_OVERRIDE".to_owned(), runtime_model.clone()));
    }
    let runtime_secret = provider_secret
        .as_ref()
        .map(|secret| {
            runtime_provider_secret_file(
                view.identity().uid(),
                view.identity().gid(),
                &config.agent,
                secret,
            )
        })
        .transpose()?;
    if let Some(secret) = runtime_secret.as_ref() {
        runtime_env.extend(secret.env());
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
            agent_name: view.agent_name(),
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Bwrap {
                program: Path::new(BWRAP_PROGRAM),
                mount_table: view.mount_table(),
            },
        },
    );
    repair_agent_session_permissions(&session_root, view.identity().uid(), view.identity().gid())?;
    result
        .map(|_response| ())
        .map_err(|error| format!("socket runtime {}: {}", error.errno(), config.agent))
}

fn runtime_model_and_secret(
    source: &Path,
    requested_model: &str,
) -> Result<(String, Option<cortexfs::ProviderSystemSecret>), cortexfs::ProviderSystemSecretError> {
    let requested_secret =
        cortexfs::read_provider_system_secret_for_model(source, requested_model)?;
    if requested_secret.is_some() {
        return Ok((requested_model.to_owned(), requested_secret));
    }
    let resolved = resolved_runtime_model(source, requested_model);
    let Some(local) = local_runtime_model_counterpart(&resolved) else {
        return Ok((requested_model.to_owned(), None));
    };
    let local_secret = cortexfs::read_provider_system_secret_for_model(source, &local)?;
    if local_secret.is_some() {
        Ok((local, local_secret))
    } else {
        Ok((requested_model.to_owned(), None))
    }
}

fn resolved_runtime_model(source: &Path, model: &str) -> String {
    if !matches!(model, "main" | "helper") {
        return model.to_owned();
    }
    let Ok(target) = fs::read_link(source.join("model").join(model)) else {
        return model.to_owned();
    };
    let target = target.to_string_lossy();
    target
        .strip_prefix("/ctx/model/")
        .or_else(|| target.strip_prefix("model/"))
        .unwrap_or(&target)
        .to_owned()
}

fn local_runtime_model_counterpart(model: &str) -> Option<String> {
    let (provider, name) = model.split_once('/')?;
    if provider == "local" || name.is_empty() {
        None
    } else {
        Some(format!("local/{name}"))
    }
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
    fchown(fd, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid)))
        .map_err(|error| format!("cannot chown session path {label}: {error}"))?;
    let mode = if is_dir { 0o700 } else { 0o600 };
    fchmod(fd, Mode::from_bits_truncate(mode))
        .map_err(|error| format!("cannot chmod session path {label}: {error}"))?;
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

struct RuntimeProviderSecretFile {
    dir_fd: OwnedFd,
    file_name: String,
    path: PathBuf,
    provider: String,
    account: String,
}

impl RuntimeProviderSecretFile {
    fn env(&self) -> [(String, String); 3] {
        [
            (
                "CTX_PROVIDER_SECRET_PATH".to_owned(),
                self.path.display().to_string(),
            ),
            (
                "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
                self.provider.clone(),
            ),
            ("CTX_PROVIDER_SECRET_SLOT".to_owned(), self.account.clone()),
        ]
    }
}

impl Drop for RuntimeProviderSecretFile {
    fn drop(&mut self) {
        let _ignored = unlinkat(
            &self.dir_fd,
            self.file_name.as_str(),
            UnlinkatFlags::NoRemoveDir,
        );
    }
}

fn runtime_provider_secret_file(
    uid: u32,
    gid: u32,
    agent: &str,
    secret: &cortexfs::ProviderSystemSecret,
) -> Result<RuntimeProviderSecretFile, String> {
    let dir = PathBuf::from(format!("/run/user/{uid}/cortexfs/credentials"));
    let dir_fd = open_runtime_credential_dir(uid, gid)?;
    let file_name = safe_runtime_credential_name(agent, secret.account())?;
    let path = dir.join(&file_name);
    let mut file = create_runtime_credential_file(&dir_fd, &file_name, uid, gid)?;
    file.write_all(secret.secret().as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|error| format!("cannot write runtime credential file: {error}"))?;
    Ok(RuntimeProviderSecretFile {
        dir_fd,
        file_name,
        path,
        provider: secret.provider().to_owned(),
        account: secret.account().to_owned(),
    })
}

fn open_runtime_credential_dir(uid: u32, gid: u32) -> Result<OwnedFd, String> {
    let user_dir = PathBuf::from(format!("/run/user/{uid}"));
    let user_fd = open_dir_no_follow(&user_dir)?;
    mkdirat_ignore_exists(&user_fd, "cortexfs", 0o700)?;
    let cortex_fd = open_child_dir_no_follow(&user_fd, "cortexfs")?;
    repair_runtime_credential_dir(&cortex_fd, uid, gid)?;
    mkdirat_ignore_exists(&cortex_fd, "credentials", 0o700)?;
    let credentials_fd = open_child_dir_no_follow(&cortex_fd, "credentials")?;
    repair_runtime_credential_dir(&credentials_fd, uid, gid)?;
    Ok(credentials_fd)
}

fn repair_runtime_credential_dir(dir_fd: &OwnedFd, uid: u32, gid: u32) -> Result<(), String> {
    fchown(dir_fd, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid)))
        .map_err(|error| format!("cannot chown runtime credential dir: {error}"))?;
    fchmod(dir_fd, Mode::from_bits_truncate(0o700))
        .map_err(|error| format!("cannot chmod runtime credential dir: {error}"))
}

fn create_runtime_credential_file(
    dir_fd: &OwnedFd,
    file_name: &str,
    uid: u32,
    gid: u32,
) -> Result<File, String> {
    // Remove only the entry inside the already-opened credentials directory. If a
    // user pre-created a symlink at the predictable path, this unlinks the
    // symlink itself instead of following it. The replacement is then created
    // with O_NOFOLLOW and owned by the target agent uid/gid via fchown on the fd.
    if let Err(error) = unlinkat(dir_fd, file_name, UnlinkatFlags::NoRemoveDir)
        && error != Errno::ENOENT
    {
        return Err(format!("cannot replace runtime credential file: {error}"));
    }
    let fd = openat(
        dir_fd,
        file_name,
        OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_WRONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(|error| format!("cannot create runtime credential file: {error}"))?;
    fchown(&fd, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid)))
        .map_err(|error| format!("cannot chown runtime credential file: {error}"))?;
    Ok(File::from(fd))
}

fn open_dir_no_follow(path: &Path) -> Result<OwnedFd, String> {
    open(
        path,
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("cannot open runtime credential dir: {error}"))
}

fn open_child_dir_no_follow(parent: &OwnedFd, name: &str) -> Result<OwnedFd, String> {
    openat(
        parent,
        name,
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("cannot open runtime credential dir: {error}"))
}

fn mkdirat_ignore_exists(parent: &OwnedFd, name: &str, mode: u32) -> Result<(), String> {
    if let Err(error) = mkdirat(parent, name, Mode::from_bits_truncate(mode)) {
        if error == Errno::EEXIST {
            return Ok(());
        }
        return Err(format!("cannot create runtime credential dir: {error}"));
    }
    Ok(())
}

fn safe_runtime_credential_name(agent: &str, account: &str) -> Result<String, String> {
    if !is_object_name(agent) || !is_object_name(account) {
        return Err("runtime credential path components must be object names".to_owned());
    }
    Ok(format!("{agent}-provider-{account}"))
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
