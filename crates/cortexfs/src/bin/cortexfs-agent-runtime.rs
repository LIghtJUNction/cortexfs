use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cortexfs::{
    AgentExecutableSocketRuntime, SocketPeerPolicy, derive_agent_runtime_view,
    serve_agent_executable_socket_listener_once,
};
use listenfd::ListenFd;
use nix::unistd::{Gid, Uid, getuid, setgid, setgroups, setuid};

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
    drop_to_agent_identity(view.identity())?;
    let agent_executable = config.source.join("agent").join(&config.agent);
    serve_agent_executable_socket_listener_once(
        &listener,
        Some(peer_policy),
        AgentExecutableSocketRuntime {
            ctx_root: Path::new(cortexfs::CTX_ROOT),
            source_root: &config.source,
            session_root: &session_root,
            default_cwd: &default_cwd,
            model: Some(view.model()),
            agent_name: view.agent_name(),
            agent_executable: &agent_executable,
        },
    )
    .map(|_response| ())
    .map_err(|error| format!("socket runtime {}: {}", error.errno(), config.agent))
}

fn drop_to_agent_identity(identity: &cortexfs::AgentUnixIdentity) -> Result<(), String> {
    if getuid().as_raw() != 0 {
        if getuid().as_raw() == identity.uid() {
            return Ok(());
        }
        return Err(format!(
            "agent runtime must run as root or uid {}; current uid is {}",
            identity.uid(),
            getuid().as_raw()
        ));
    }

    let groups = identity
        .groups()
        .iter()
        .copied()
        .map(Gid::from_raw)
        .collect::<Vec<_>>();
    setgroups(&groups).map_err(|error| format!("drop supplementary groups: {error}"))?;
    setgid(Gid::from_raw(identity.gid())).map_err(|error| format!("drop gid: {error}"))?;
    setuid(Uid::from_raw(identity.uid())).map_err(|error| format!("drop uid: {error}"))?;
    Ok(())
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
