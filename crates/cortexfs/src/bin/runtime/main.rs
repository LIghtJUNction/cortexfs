#![forbid(unsafe_code)]
#![expect(
    clippy::allow_attributes,
    reason = "allow target-specific lint exceptions"
)]
#![allow(
    unfulfilled_lint_expectations,
    reason = "expected target-specific lint results"
)]
#![expect(
    clippy::wildcard_imports,
    reason = "uniform submodules with wildcard imports"
)]
#![expect(clippy::redundant_pub_crate, reason = "submodule visibility alignment")]
#![expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "internal structs with scoped fields"
)]
#![expect(clippy::module_inception, reason = "allow submodule self name")]

use cortexfs::{
    AgentExecutableSocketRuntime, AgentStopHandler, MountTable, NetworkConnectAuthority,
    PreparedAgentStop, RunEnvironment, SocketPeerPolicy, SocketRuntimeError,
    authorize_network_connect, derive_agent_runtime_view,
    serve_agent_executable_socket_listener_once_with_stop,
};
use listenfd::ListenFd;
use nix::sys::stat::{Mode, fchmod};
use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
};

const DEFAULT_SOURCE: &str = cortexfs_paths::SYSTEM_STORAGE_CURRENT;
const BWRAP_PROGRAM: &str = cortexfs::support::command::BWRAP;
const RUN_CONTROL_DIR: &str = cortexfs_paths::SYSTEM_CONTROL_DIR;

use cortexfs::cli::stderr::write_error;

pub(crate) fn main() -> ExitCode {
    if let Err(error) = run(env::args_os().skip(1).collect()) {
        let _ignored = write_error(&format!("cortexfs-agent-runtime: {error}"));
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

pub(crate) fn run(args: Vec<OsString>) -> Result<(), String> {
    let mut config = RuntimeConfig::parse(args)?;
    config.source = cortexfs::pin_storage_source(&config.source)
        .map_err(|error| format!("invalid source root: {error}"))?;
    let (action, result) = match config.mode {
        RuntimeMode::Serve => return serve(&config),
        RuntimeMode::PrepareSocketAlias => (
            "prepare",
            cortexfs::agent::launch::prepare_system_agent_alias(&config.source, &config.agent),
        ),
        RuntimeMode::CleanupSocketAlias => (
            "cleanup",
            cortexfs::agent::launch::cleanup_system_agent_alias(&config.source, &config.agent),
        ),
    };
    result
        .map(|_changed| ())
        .map_err(|error| format!("{action} socket alias: {error}"))
}

fn serve(config: &RuntimeConfig) -> Result<(), String> {
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
    let session_root =
        cortexfs_paths::agent_sessions_from_home_path(view.ctx_home(), view.agent_name());
    let default_cwd = view.cwd().display().to_string();
    let peer_policy = SocketPeerPolicy::uid_or_root(view.identity().uid());
    let model = view.model();
    let network_allowed = authorize_network_connect(
        "default",
        NetworkConnectAuthority::new(view.policy_subject(), view.policy()),
    )
    .is_ok();
    let mut env = view.env().to_vec();
    env.extend(provider_runtime_env(&config.source, model)?);
    let executable =
        cortexfs::agent::resolve_agent_loop_executable_for_agent(&config.source, &config.agent)
            .map_err(|_error| "cannot resolve agent loop executable".to_owned())?;
    let control_dir = Path::new(RUN_CONTROL_DIR);
    #[expect(
        clippy::create_dir,
        reason = "single private leaf does not traverse parents"
    )]
    match fs::create_dir(control_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("cannot create run control directory: {error}")),
    }
    let control_fd = cortexfs::support::plain::open_plain_directory(control_dir)
        .map_err(|error| format!("cannot open run control directory: {error}"))?;
    fchmod(&control_fd, Mode::from_bits_truncate(0o711))
        .map_err(|error| format!("cannot secure run control directory: {error}"))?;
    let stop = RuntimeStopHandler(
        config.source.clone(),
        view.owner(),
        view.agent_name().to_owned(),
    );
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &config.source,
        source_root: &config.source,
        identity: view.identity(),
        env: &env,
        session_root: &session_root,
        default_cwd: &default_cwd,
        model: Some(model),
        network_allowed,
        agent_name: view.agent_name(),
        agent_executable: &executable,
        environment: runtime_agent_environment(view.mount_table(), control_dir),
    };
    // Keep v1 one-shot admission until session ownership and cancellation routing
    // are independent of the per-Agent current-session index.
    serve_agent_executable_socket_listener_once_with_stop(
        &listener,
        Some(peer_policy),
        runtime,
        Some(&stop),
    )
    .map(|_response| ())
    .map_err(|error| {
        format!(
            "socket runtime {} {error:?}: {}",
            error.errno(),
            config.agent
        )
    })
}
struct RuntimeStopHandler(PathBuf, u32, String);
struct RuntimePreparedStop(cortexfs::agent::stop::StopPlan);
impl AgentStopHandler for RuntimeStopHandler {
    fn preflight(
        &self,
        agent: &str,
        peer_uid: u32,
    ) -> Result<Box<dyn PreparedAgentStop>, SocketRuntimeError> {
        let context = cortexfs::agent::stop::StopContext {
            source: self.0.clone(),
            owner_uid: self.1,
            peer_uid,
            runtime_agent: self.2.clone(),
        };
        cortexfs::agent::stop::plan_stop(&context, agent)
            .map(|plan| -> Box<dyn PreparedAgentStop> { Box::new(RuntimePreparedStop(plan)) })
            .map_err(|error| {
                let _ignored =
                    write_error(&format!("cortexfs-agent-runtime: stop preflight: {error}"));
                SocketRuntimeError::CannotRunAgent
            })
    }
}
impl PreparedAgentStop for RuntimePreparedStop {
    fn execute(self: Box<Self>) -> Result<(), SocketRuntimeError> {
        cortexfs::agent::stop::execute_stop(self.0)
            .map_err(|_error| SocketRuntimeError::PostAcceptStop)
    }
}
pub(crate) fn runtime_agent_environment<'a>(
    mount_table: &'a MountTable,
    control_dir: &'a Path,
) -> RunEnvironment<'a> {
    RunEnvironment::Sandbox {
        program: Path::new(BWRAP_PROGRAM),
        mount_table,
        control_dir: Some(control_dir),
    }
}
fn provider_runtime_env(source: &Path, model: &str) -> Result<Vec<(String, String)>, String> {
    if cortexfs::selected_model_provider(source, model).as_deref() == Some("codex") {
        let credential = cortexfs::resolve_codex_system()
            .map_err(|_error| "codex system credential refresh failed".to_owned())?
            .ok_or_else(|| {
                "missing codex system credential; run sudo ctx provider oauth login codex"
                    .to_owned()
            })?;
        return Ok(secret_runtime_env(
            credential.0,
            "codex".to_owned(),
            "default".to_owned(),
            credential.1,
        ));
    }
    let Some(secret) = cortexfs::read_provider_system_secret_for_model(source, model)
        .map_err(|_error| "system provider credential unavailable".to_owned())?
    else {
        return Ok(Vec::new());
    };
    Ok(secret_runtime_env(
        secret.secret().to_owned(),
        secret.provider().to_owned(),
        secret.account().to_owned(),
        String::new(),
    ))
}
fn secret_runtime_env(
    token: String,
    provider: String,
    slot: String,
    account: String,
) -> Vec<(String, String)> {
    [
        ("CTX_PROVIDER_SECRET_VALUE", token),
        ("CTX_PROVIDER_SECRET_PROVIDER", provider),
        ("CTX_PROVIDER_SECRET_SLOT", slot),
        ("CTX_PROVIDER_SECRET_ACCOUNT_ID", account),
    ]
    .map(|(name, value)| (name.to_owned(), value))
    .to_vec()
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeMode {
    Serve,
    PrepareSocketAlias,
    CleanupSocketAlias,
}
#[derive(Debug, Eq, PartialEq)]
struct RuntimeConfig {
    source: PathBuf,
    agent: String,
    mode: RuntimeMode,
}
impl RuntimeConfig {
    fn parse(args: Vec<OsString>) -> Result<Self, String> {
        let mut config = Self {
            source: PathBuf::from(DEFAULT_SOURCE),
            agent: String::new(),
            mode: RuntimeMode::Serve,
        };
        let mut agent_set = false;
        let mut positional = false;
        let mut values = args.into_iter();
        while let Some(value) = values.next() {
            if value == "--source" || value == "-s" {
                config.source = PathBuf::from(values.next().ok_or("--source requires a path")?);
            } else if value == "--agent" || value == "-a" {
                config.agent = os_string(values.next().ok_or("--agent requires a name")?)?;
                agent_set = true;
            } else if value == "--prepare-socket-alias" || value == "--cleanup-socket-alias" {
                if config.mode != RuntimeMode::Serve {
                    return Err("runtime modes are mutually exclusive".to_owned());
                }
                config.mode = if value == "--prepare-socket-alias" {
                    RuntimeMode::PrepareSocketAlias
                } else {
                    RuntimeMode::CleanupSocketAlias
                };
            } else if value == "--help" || value == "-h" {
                return Err(usage());
            } else if !agent_set {
                config.agent = os_string(value)?;
                agent_set = true;
                positional = true;
            } else {
                return Err("unexpected extra argument".to_owned());
            }
        }
        if !agent_set {
            return Err(usage());
        }
        if config.mode != RuntimeMode::Serve && positional {
            return Err("internal socket alias modes require --agent".to_owned());
        }
        Ok(config)
    }
}
pub(crate) fn os_string(value: OsString) -> Result<String, String> {
    value
        .into_string()
        .map_err(|value| format!("arguments must be valid UTF-8: {}", value.to_string_lossy()))
}
pub(crate) fn usage() -> String {
    "usage: cortexfs-agent-runtime [--source CTX_SOURCE] --agent AGENT [--prepare-socket-alias|--cleanup-socket-alias]".to_owned()
}

#[cfg(test)]
mod tests;
