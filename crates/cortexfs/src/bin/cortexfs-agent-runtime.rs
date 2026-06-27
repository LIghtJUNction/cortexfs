use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cortexfs::{
    AgentExecutableSocketRuntime, SocketPeerPolicy, derive_agent_runtime_view,
    serve_agent_executable_socket_listener_once,
};
use listenfd::ListenFd;
use nix::errno::Errno;
use nix::fcntl::{OFlag, open, openat};
use nix::sys::stat::{Mode, fchmod, mkdirat};
use nix::unistd::{Gid, Uid, chown};
use nix::unistd::{UnlinkatFlags, fchown, unlinkat};

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
    let provider_secret =
        cortexfs::read_provider_system_secret_for_model(&config.source, view.model())
            .map_err(|_error| format!("provider secret unavailable: {}", view.model()))?;
    let mut runtime_env = view.env().to_vec();
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
            model: Some(view.model()),
            agent_name: view.agent_name(),
            agent_executable: &agent_executable,
        },
    );
    repair_agent_session_permissions(&session_root, view.identity().uid(), view.identity().gid())?;
    if let Some(secret) = runtime_secret.as_ref() {
        secret.cleanup();
    }
    result
        .map(|_response| ())
        .map_err(|error| format!("socket runtime {}: {}", error.errno(), config.agent))
}

fn runtime_agent_executable(ctx_root: &Path, agent: &str) -> PathBuf {
    ctx_root.join("agent").join(agent)
}

fn repair_agent_session_permissions(session_root: &Path, uid: u32, gid: u32) -> Result<(), String> {
    if !session_root.exists() {
        return Ok(());
    }
    repair_path_permissions(session_root, uid, gid)
}

fn repair_path_permissions(path: &Path, uid: u32, gid: u32) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect session path {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    chown(path, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid)))
        .map_err(|error| format!("cannot chown session path {}: {error}", path.display()))?;
    let mode = if metadata.is_dir() { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("cannot chmod session path {}: {error}", path.display()))?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)
            .map_err(|error| format!("cannot read session dir {}: {error}", path.display()))?
        {
            let entry = entry.map_err(|error| format!("cannot read session dir entry: {error}"))?;
            repair_path_permissions(&entry.path(), uid, gid)?;
        }
    }
    Ok(())
}

struct RuntimeProviderSecretFile {
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

    fn cleanup(&self) {
        let _ignored = fs::remove_file(&self.path);
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
    if agent.contains('/') || account.contains('/') {
        return Err("runtime credential path components must not contain '/'".to_owned());
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
