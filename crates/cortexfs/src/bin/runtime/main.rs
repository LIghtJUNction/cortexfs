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

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cortexfs::{
    AgentExecutableSocketExecution, AgentExecutableSocketRuntime, AgentStopHandler, MountTable,
    PolicyObjectClass, PolicyPermission, PreparedAgentStop, SocketPeerPolicy, SocketRuntimeError,
    derive_agent_runtime_view, serve_agent_executable_socket_listener_once_with_stop,
};
use listenfd::ListenFd;
use nix::sys::stat::{Mode, fchmod};

const DEFAULT_SOURCE: &str = "/var/lib/cortexfs/storage/current";
const BWRAP_PROGRAM: &str = cortexfs::support::command::BWRAP;
const RUN_CONTROL_DIR: &str = "/run/cortexfs/control";

pub(crate) use cortexfs::cli::stderr;
pub(crate) use stderr::*;

pub(crate) fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = write_error(&format!("cortexfs-agent-runtime: {error}"));
            ExitCode::from(2)
        }
    }
}

pub(crate) fn run(args: Vec<OsString>) -> Result<(), String> {
    let mut config = RuntimeConfig::parse(args)?;
    config.source = cortexfs::pin_storage_source(&config.source)
        .map_err(|error| format!("invalid source root: {error}"))?;
    match config.mode {
        RuntimeMode::PrepareSocketAlias => {
            return cortexfs::agent::launch::prepare_system_agent_alias(
                &config.source,
                &config.agent,
            )
            .map(|_changed| ())
            .map_err(|error| format!("prepare socket alias: {error}"));
        }
        RuntimeMode::CleanupSocketAlias => {
            return cortexfs::agent::launch::cleanup_system_agent_alias(
                &config.source,
                &config.agent,
            )
            .map(|_changed| ())
            .map_err(|error| format!("cleanup socket alias: {error}"));
        }
        RuntimeMode::Serve => {}
    }
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

    let session_root = view
        .ctx_home()
        .join("agent")
        .join(view.agent_name())
        .join("session");
    let default_cwd = view.cwd().display().to_string();
    let peer_policy = SocketPeerPolicy::uid_or_root(view.identity().uid());
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
    runtime_env.extend(provider_runtime_env(&config.source, &runtime_model)?);
    let agent_executable = config.source.join("agent").join(&config.agent);
    let control_dir = Path::new(RUN_CONTROL_DIR);
    #[expect(
        clippy::create_dir,
        reason = "single private leaf must not recursively create or traverse parents"
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
    let stop = RuntimeStopHandler {
        source: config.source.clone(),
        owner_uid: view.owner(),
        runtime_agent: view.agent_name().to_owned(),
    };
    let result = serve_agent_executable_socket_listener_once_with_stop(
        &listener,
        Some(peer_policy),
        AgentExecutableSocketRuntime {
            ctx_root: &config.source,
            source_root: &config.source,
            identity: view.identity(),
            env: &runtime_env,
            session_root: &session_root,
            default_cwd: &default_cwd,
            model: Some(&runtime_model),
            network_allowed,
            agent_name: view.agent_name(),
            agent_executable: &agent_executable,
            execution: runtime_agent_execution(view.mount_table(), control_dir),
        },
        Some(&stop),
    );
    result.map(|_response| ()).map_err(|error| {
        format!(
            "socket runtime {} {error:?}: {}",
            error.errno(),
            config.agent
        )
    })
}

struct RuntimeStopHandler {
    source: PathBuf,
    owner_uid: u32,
    runtime_agent: String,
}

struct RuntimePreparedStop(cortexfs::agent::stop::ConcreteStopPlan);

impl AgentStopHandler for RuntimeStopHandler {
    fn preflight(
        &self,
        agent: &str,
        peer_uid: u32,
    ) -> Result<Box<dyn PreparedAgentStop>, SocketRuntimeError> {
        let context = cortexfs::agent::stop::StopContext {
            source: self.source.clone(),
            owner_uid: self.owner_uid,
            peer_uid,
            runtime_agent: self.runtime_agent.clone(),
        };
        cortexfs::agent::stop::plan_stop(&context, agent)
            .map(|plan| {
                let prepared: Box<dyn PreparedAgentStop> = Box::new(RuntimePreparedStop(plan));
                prepared
            })
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

pub(crate) fn runtime_agent_execution<'a>(
    mount_table: &'a MountTable,
    control_dir: &'a Path,
) -> AgentExecutableSocketExecution<'a> {
    AgentExecutableSocketExecution::Bwrap {
        program: Path::new(BWRAP_PROGRAM),
        mount_table,
        control_dir: Some(control_dir),
    }
}

pub(crate) fn runtime_model(_source: &Path, requested_model: &str) -> String {
    requested_model.to_owned()
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
        let mut source = PathBuf::from(DEFAULT_SOURCE);
        let mut agent = None;
        let mut positional_agent = false;
        let mut mode = RuntimeMode::Serve;
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
            if value == "--prepare-socket-alias" {
                if mode != RuntimeMode::Serve {
                    return Err("runtime modes are mutually exclusive".to_owned());
                }
                mode = RuntimeMode::PrepareSocketAlias;
                continue;
            }
            if value == "--cleanup-socket-alias" {
                if mode != RuntimeMode::Serve {
                    return Err("runtime modes are mutually exclusive".to_owned());
                }
                mode = RuntimeMode::CleanupSocketAlias;
                continue;
            }
            if value == "--help" || value == "-h" {
                return Err(usage());
            }
            if agent.is_none() {
                agent = Some(os_string(value)?);
                positional_agent = true;
                continue;
            }
            return Err("unexpected extra argument".to_owned());
        }

        let Some(agent) = agent else {
            return Err(usage());
        };
        if mode != RuntimeMode::Serve && positional_agent {
            return Err("internal socket alias modes require --agent".to_owned());
        }
        Ok(Self {
            source,
            agent,
            mode,
        })
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
