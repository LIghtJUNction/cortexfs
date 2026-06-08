#![forbid(unsafe_code)]

use cortex_core::{ApiFormat, ProviderId, ThreadId};
use cortex_providers::{InMemoryProvider, ProviderResponse};
use cortex_store::{InMemoryStore, RequestId, Store, ThreadSnapshot};
use cortexd::{ExecutionPlane, LocalApiEndpoint, LocalApiRequest};
use std::io::Write as _;
use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let command = Command::from_env();
    match run(command) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            print_error(&error);
            std::process::ExitCode::FAILURE
        }
    }
}

#[expect(clippy::print_stderr, reason = "CLI errors must be visible to users")]
fn print_error(error: &CliError) {
    eprintln!("{error}");
}

fn print_output(output: &str) -> Result<(), CliError> {
    std::io::stdout()
        .write_all(output.as_bytes())
        .map_err(CliError::Io)
}

/// Top-level `CortexFS` CLI command.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Command {
    /// Initialize user configuration and state directories.
    Init(InitCommand),
    /// Run the Cortex daemon execution plane.
    Daemon(DaemonCommand),
    /// Mount the FUSE projection.
    Mount(MountCommand),
    /// Report daemon and mount status.
    Status(StatusCommand),
    /// Reject invalid command arguments.
    Invalid(InvalidCommand),
    /// Reject an unsupported command.
    Unknown(UnknownCommand),
}

impl Command {
    fn from_env() -> Self {
        Self::parse(std::env::args_os().skip(1))
    }

    fn parse(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Self {
        let mut arguments = arguments.into_iter();
        let Some(command) = arguments.next() else {
            return Self::Status(StatusCommand);
        };

        match command.to_string_lossy().as_ref() {
            "init" => match parse_no_arguments(arguments, "init") {
                Ok(()) => Self::Init(InitCommand),
                Err(error) => Self::Invalid(error),
            },
            "daemon" => match DaemonCommand::parse(arguments) {
                Ok(command) => Self::Daemon(command),
                Err(error) => Self::Invalid(error),
            },
            "mount" => match MountCommand::parse(arguments) {
                Ok(command) => Self::Mount(command),
                Err(error) => Self::Invalid(error),
            },
            "status" => match parse_no_arguments(arguments, "status") {
                Ok(()) => Self::Status(StatusCommand),
                Err(error) => Self::Invalid(error),
            },
            unknown => Self::Unknown(UnknownCommand::new(unknown)),
        }
    }
}

fn parse_no_arguments(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
    command: &str,
) -> Result<(), InvalidCommand> {
    if let Some(argument) = arguments.into_iter().next() {
        return Err(InvalidCommand::new(format!(
            "unexpected {command} argument: {}",
            argument.to_string_lossy()
        )));
    }
    Ok(())
}

/// Placeholder for `cortex init`.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct InitCommand;

/// Run the daemon execution plane.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DaemonCommand {
    once: bool,
    method: String,
    endpoint: String,
    body: String,
    request_id: String,
    provider: Option<String>,
    model: Option<String>,
    thread: Option<String>,
}

impl DaemonCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut command = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.to_string_lossy().as_ref() {
                "--once" => command.once = true,
                "--endpoint" => {
                    command.endpoint = parse_required_argument(&mut arguments, "--endpoint")?;
                }
                "--method" => {
                    command.method = parse_required_argument(&mut arguments, "--method")?;
                }
                "--body" => {
                    command.body = parse_required_value(&mut arguments, "--body")?;
                }
                "--request-id" => {
                    command.request_id = parse_required_argument(&mut arguments, "--request-id")?;
                }
                "--provider" => {
                    command.provider = Some(parse_required_argument(&mut arguments, "--provider")?);
                }
                "--model" => {
                    command.model = Some(parse_required_argument(&mut arguments, "--model")?);
                }
                "--thread" => {
                    command.thread = Some(parse_required_argument(&mut arguments, "--thread")?);
                }
                unknown => {
                    return Err(InvalidCommand::new(format!(
                        "unknown daemon argument: {unknown}"
                    )));
                }
            }
        }
        Ok(command)
    }
}

impl Default for DaemonCommand {
    fn default() -> Self {
        Self {
            once: false,
            method: "POST".to_owned(),
            endpoint: "/v1/chat/completions".to_owned(),
            body: r#"{"messages":[{"role":"user","content":"Reply with cortexfs-daemon-ok"}]}"#
                .to_owned(),
            request_id: "daemon-once".to_owned(),
            provider: None,
            model: None,
            thread: None,
        }
    }
}

fn parse_required_argument(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    flag: &str,
) -> Result<String, InvalidCommand> {
    let value = parse_required_value(arguments, flag)?;
    if value.starts_with("--") {
        return Err(InvalidCommand::new(format!(
            "missing value for daemon argument: {flag}"
        )));
    }
    Ok(value)
}

fn parse_required_value(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    flag: &str,
) -> Result<String, InvalidCommand> {
    let Some(value) = arguments.next() else {
        return Err(InvalidCommand::new(format!(
            "missing value for daemon argument: {flag}"
        )));
    };
    Ok(value.to_string_lossy().into_owned())
}

/// Placeholder for `cortex mount`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MountCommand {
    mountpoint: PathBuf,
    multi_user: bool,
}

impl MountCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut mountpoint = PathBuf::from("mnt/cortex");
        let mut multi_user = false;
        let mut explicit_mountpoint = false;

        for argument in arguments {
            if argument == "--multi-user" {
                multi_user = true;
            } else if argument.to_string_lossy().starts_with("--") {
                return Err(InvalidCommand::new(format!(
                    "unknown mount argument: {}",
                    argument.to_string_lossy()
                )));
            } else {
                if explicit_mountpoint {
                    return Err(InvalidCommand::new(format!(
                        "unexpected extra mountpoint: {}",
                        argument.to_string_lossy()
                    )));
                }
                mountpoint = PathBuf::from(argument);
                explicit_mountpoint = true;
            }
        }

        Ok(Self {
            mountpoint,
            multi_user,
        })
    }

    fn fuse_config(&self) -> cortexfs::FuseConfig {
        let options = if self.multi_user {
            cortexfs::MountOptions::multi_user(self.mountpoint.clone())
        } else {
            cortexfs::MountOptions::new(self.mountpoint.clone())
        };
        cortexfs::FuseConfig::new(options)
    }
}

/// Placeholder for `cortex status`.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct StatusCommand;

impl StatusCommand {
    fn render() -> String {
        let uid = current_uid();
        format!(
            "status=ready\nabi=cortexfs.design.v0\nplatform=linux\nrecommended_mount=/ctx\nhome=/ctx/home/{uid}\ndefault_test_mount=tests/mounts/cortexfs\nlive_test_provider=provider-neutral\nlive_test_model=smollm2:135m\n"
        )
    }
}

fn current_uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| parse_effective_uid(&status))
        .unwrap_or_default()
}

fn parse_effective_uid(status: &str) -> Option<u32> {
    let line = status.lines().find(|line| line.starts_with("Uid:"))?;
    let mut fields = line.split_whitespace();
    let _label = fields.next()?;
    let _real_uid = fields.next()?;
    fields.next()?.parse().ok()
}

/// Invalid command arguments.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InvalidCommand {
    message: String,
}

impl InvalidCommand {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Unsupported command.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UnknownCommand {
    name: String,
}

impl UnknownCommand {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// Error returned by command placeholders.
#[derive(Debug)]
pub enum CliError {
    /// The requested behavior is intentionally not implemented yet.
    NotImplemented(&'static str),
    /// Core identifier or API format validation failed.
    Validation(cortex_core::ValidationError),
    /// Daemon execution failed.
    Execution(cortexd::ExecutionError),
    /// Store read/write failed.
    Store(cortex_store::StoreError),
    /// Local API front-door normalization failed.
    LocalApi(cortexd::LocalApiError),
    /// Command arguments are outside the CLI ABI.
    InvalidCommand(String),
    /// The command is not part of the CLI ABI.
    UnknownCommand(String),
    /// The FUSE projection returned an error.
    Mount(cortexfs::MountError),
    /// Writing CLI output failed.
    Io(std::io::Error),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::NotImplemented(command) => write!(f, "{command} is not implemented"),
            Self::Validation(ref error) => error.fmt(f),
            Self::Execution(ref error) => error.fmt(f),
            Self::Store(ref error) => error.fmt(f),
            Self::LocalApi(ref error) => error.fmt(f),
            Self::InvalidCommand(ref message) => write!(f, "invalid command: {message}"),
            Self::UnknownCommand(ref command) => write!(f, "unknown command: {command}"),
            Self::Mount(ref error) => error.fmt(f),
            Self::Io(ref error) => write!(f, "I/O error: {error}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<cortexfs::MountError> for CliError {
    fn from(error: cortexfs::MountError) -> Self {
        Self::Mount(error)
    }
}

impl From<cortex_core::ValidationError> for CliError {
    fn from(error: cortex_core::ValidationError) -> Self {
        Self::Validation(error)
    }
}

impl From<cortexd::ExecutionError> for CliError {
    fn from(error: cortexd::ExecutionError) -> Self {
        Self::Execution(error)
    }
}

impl From<cortex_store::StoreError> for CliError {
    fn from(error: cortex_store::StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<cortexd::LocalApiError> for CliError {
    fn from(error: cortexd::LocalApiError) -> Self {
        Self::LocalApi(error)
    }
}

/// Executes a parsed command.
///
/// # Errors
///
/// Returns [`CliError::NotImplemented`] for command placeholders and propagates
/// mount scaffolding errors.
pub fn run(command: Command) -> Result<(), CliError> {
    match command {
        Command::Init(_command) => Err(CliError::NotImplemented("init")),
        Command::Daemon(command) => {
            if command.once {
                return run_daemon_once(command);
            }
            Err(CliError::NotImplemented("daemon"))
        }
        Command::Mount(command) => {
            let config = command.fuse_config();
            cortexfs::mount(&config)?;
            Ok(())
        }
        Command::Status(_command) => print_output(&StatusCommand::render()),
        Command::Invalid(command) => Err(CliError::InvalidCommand(command.message)),
        Command::Unknown(command) => Err(CliError::UnknownCommand(command.name)),
    }
}

fn run_daemon_once(command: DaemonCommand) -> Result<(), CliError> {
    let body = daemon_once_response(command)?;
    print_output(&body)?;
    print_output("\n")
}

fn daemon_once_response(command: DaemonCommand) -> Result<String, CliError> {
    Ok(daemon_once_result(command)?.body)
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct DaemonOnceResult {
    body: String,
    thread: Option<ThreadSnapshot>,
}

fn daemon_once_result(command: DaemonCommand) -> Result<DaemonOnceResult, CliError> {
    let provider = InMemoryProvider::new(
        ProviderId::new("daemon-local")?,
        vec![
            ApiFormat::OpenAiChat,
            ApiFormat::OpenAiResponses,
            ApiFormat::AnthropicMessages,
            ApiFormat::GoogleGenerateContent,
        ],
    )
    .with_response(
        ApiFormat::OpenAiChat,
        ProviderResponse::new(
            ApiFormat::OpenAiChat,
            r#"{"id":"chatcmpl-daemon-once","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"cortexfs-daemon-ok"},"finish_reason":"stop"}]}"#,
        ),
    )
    .with_response(
        ApiFormat::OpenAiResponses,
        ProviderResponse::new(
            ApiFormat::OpenAiResponses,
            r#"{"id":"resp-daemon-once","object":"response","output_text":"cortexfs-daemon-ok"}"#,
        ),
    )
    .with_response(
        ApiFormat::AnthropicMessages,
        ProviderResponse::new(
            ApiFormat::AnthropicMessages,
            r#"{"id":"msg-daemon-once","type":"message","role":"assistant","content":[{"type":"text","text":"cortexfs-daemon-ok"}]}"#,
        ),
    )
    .with_response(
        ApiFormat::GoogleGenerateContent,
        ProviderResponse::new(
            ApiFormat::GoogleGenerateContent,
            r#"{"candidates":[{"content":{"parts":[{"text":"cortexfs-daemon-ok"}]}}]}"#,
        ),
    )
    .with_models(vec![
        cortex_providers::ProviderModel::new(
            cortex_core::ModelId::new("daemon-openai-chat")?,
            ApiFormat::OpenAiChat,
        ),
        cortex_providers::ProviderModel::new(
            cortex_core::ModelId::new("daemon-openai-responses")?,
            ApiFormat::OpenAiResponses,
        ),
        cortex_providers::ProviderModel::new(
            cortex_core::ModelId::new("daemon-anthropic-messages")?,
            ApiFormat::AnthropicMessages,
        ),
        cortex_providers::ProviderModel::new(
            cortex_core::ModelId::new("daemon-google-generate-content")?,
            ApiFormat::GoogleGenerateContent,
        ),
    ]);
    let mut plane = ExecutionPlane::new(InMemoryStore::new(), provider);
    let thread = command.thread.map(ThreadId::new).transpose()?;
    let mut request = LocalApiRequest::new(
        RequestId::new(command.request_id),
        LocalApiEndpoint::parse(&command.method, &command.endpoint)?,
        command.body,
    );
    if let Some(provider) = command.provider {
        request = request.with_provider(ProviderId::new(provider)?);
    }
    if let Some(model) = command.model {
        request = request.with_model(cortex_core::ModelId::new(model)?);
    }
    if let Some(thread) = thread.clone() {
        request = request.with_thread(thread);
    }
    let writes_thread = request.endpoint().format().is_some();
    let response = plane.handle_local_api(request)?;
    let thread = writes_thread
        .then_some(thread)
        .flatten()
        .as_ref()
        .map(|thread| plane.store().thread_snapshot(thread))
        .transpose()?;

    Ok(DaemonOnceResult {
        body: response.body().to_owned(),
        thread,
    })
}

#[cfg(test)]
mod tests {
    use super::{Command, DaemonCommand, MountCommand, StatusCommand};
    use cortex_core::MessageRole;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn parser_defaults_to_status_without_arguments() {
        assert_eq!(Command::parse([]), Command::Status(StatusCommand));
    }

    #[test]
    fn status_renders_stable_discovery_fields() {
        let status = StatusCommand::render();

        assert!(status.contains("status=ready\n"));
        assert!(status.contains("abi=cortexfs.design.v0\n"));
        assert!(status.contains("platform=linux\n"));
        assert!(status.contains("recommended_mount=/ctx\n"));
        assert!(status.contains("home=/ctx/home/"));
        assert!(status.contains("default_test_mount=tests/mounts/cortexfs\n"));
        assert!(status.contains("live_test_provider=provider-neutral\n"));
        assert!(status.contains("live_test_model=smollm2:135m\n"));
    }

    #[test]
    fn parser_rejects_status_arguments() {
        assert_eq!(
            Command::parse([OsString::from("status"), OsString::from("--watch")]),
            Command::Invalid(super::InvalidCommand::new(
                "unexpected status argument: --watch"
            ))
        );
    }

    #[test]
    fn parser_rejects_init_arguments() {
        assert_eq!(
            Command::parse([OsString::from("init"), OsString::from("--watch")]),
            Command::Invalid(super::InvalidCommand::new(
                "unexpected init argument: --watch"
            ))
        );
    }

    #[test]
    fn parses_effective_uid_from_proc_status() {
        let status = "Name:\tcortex\nUid:\t1000\t2000\t3000\t4000\n";

        assert_eq!(super::parse_effective_uid(status), Some(2000));
    }

    #[test]
    fn parser_accepts_daemon_once_without_background_shortcut() {
        assert_eq!(
            Command::parse([OsString::from("daemon"), OsString::from("--once")]),
            Command::Daemon(DaemonCommand {
                once: true,
                ..DaemonCommand::default()
            })
        );
    }

    #[test]
    fn daemon_once_runs_execution_plane_and_prints_native_response()
    -> Result<(), Box<dyn std::error::Error>> {
        let response = super::daemon_once_response(DaemonCommand {
            once: true,
            ..DaemonCommand::default()
        })?;

        assert!(response.contains("cortexfs-daemon-ok"));
        Ok(())
    }

    #[test]
    fn parser_accepts_daemon_once_endpoint_and_body() -> Result<(), Box<dyn std::error::Error>> {
        let command = Command::parse([
            OsString::from("daemon"),
            OsString::from("--once"),
            OsString::from("--endpoint"),
            OsString::from("/v1/responses"),
            OsString::from("--method"),
            OsString::from("POST"),
            OsString::from("--body"),
            OsString::from(r#"{"input":"hello"}"#),
            OsString::from("--request-id"),
            OsString::from("local-001"),
            OsString::from("--provider"),
            OsString::from("daemon-local"),
            OsString::from("--model"),
            OsString::from("model-a"),
            OsString::from("--thread"),
            OsString::from("demo"),
        ]);

        let Command::Daemon(command) = command else {
            return Err("expected daemon command".into());
        };
        assert!(command.once);
        assert_eq!(command.method, "POST");
        assert_eq!(command.endpoint, "/v1/responses");
        assert_eq!(command.body, r#"{"input":"hello"}"#);
        assert_eq!(command.request_id, "local-001");
        assert_eq!(command.provider.as_deref(), Some("daemon-local"));
        assert_eq!(command.model.as_deref(), Some("model-a"));
        assert_eq!(command.thread.as_deref(), Some("demo"));
        Ok(())
    }

    #[test]
    fn parser_accepts_daemon_body_that_starts_with_dashes() -> Result<(), Box<dyn std::error::Error>>
    {
        let command = Command::parse([
            OsString::from("daemon"),
            OsString::from("--once"),
            OsString::from("--body"),
            OsString::from("--literal-body"),
        ]);

        let Command::Daemon(command) = command else {
            return Err("expected daemon command".into());
        };
        assert_eq!(command.body, "--literal-body");
        Ok(())
    }

    #[test]
    fn daemon_once_thread_option_updates_thread_snapshot() -> Result<(), Box<dyn std::error::Error>>
    {
        let result = super::daemon_once_result(DaemonCommand {
            once: true,
            method: "POST".to_owned(),
            endpoint: "/v1/chat/completions".to_owned(),
            body: r#"{"messages":[{"role":"user","content":"ping"}]}"#.to_owned(),
            request_id: "threaded-cli".to_owned(),
            provider: None,
            model: None,
            thread: Some("demo".to_owned()),
        })?;

        assert!(result.body.contains("cortexfs-daemon-ok"));
        let thread = result.thread.ok_or("thread snapshot should be returned")?;
        assert_eq!(thread.id().as_str(), "demo");
        assert_eq!(thread.messages().len(), 2);
        assert_eq!(
            thread.messages().first().map(cortex_core::Message::role),
            Some(MessageRole::User)
        );
        assert_eq!(
            thread.messages().first().map(cortex_core::Message::content),
            Some("ping")
        );
        assert_eq!(thread.latest(), Some("cortexfs-daemon-ok"));
        assert!(
            thread
                .fingerprint()
                .is_some_and(|fingerprint| fingerprint.starts_with("fnv1a64:"))
        );
        Ok(())
    }

    #[test]
    fn parser_rejects_unknown_daemon_argument() {
        assert_eq!(
            Command::parse([OsString::from("daemon"), OsString::from("--watch")]),
            Command::Invalid(super::InvalidCommand::new(
                "unknown daemon argument: --watch"
            ))
        );
    }

    #[test]
    fn parser_rejects_missing_daemon_argument_value() {
        assert_eq!(
            Command::parse([OsString::from("daemon"), OsString::from("--provider")]),
            Command::Invalid(super::InvalidCommand::new(
                "missing value for daemon argument: --provider"
            ))
        );
    }

    #[test]
    fn parser_rejects_daemon_argument_value_that_looks_like_flag() {
        assert_eq!(
            Command::parse([
                OsString::from("daemon"),
                OsString::from("--once"),
                OsString::from("--provider"),
                OsString::from("--model"),
                OsString::from("model-a"),
            ]),
            Command::Invalid(super::InvalidCommand::new(
                "missing value for daemon argument: --provider"
            ))
        );
    }

    #[test]
    fn daemon_once_models_endpoint_returns_model_list() -> Result<(), Box<dyn std::error::Error>> {
        let response = super::daemon_once_response(DaemonCommand {
            once: true,
            method: "GET".to_owned(),
            endpoint: "/v1/models".to_owned(),
            request_id: "models".to_owned(),
            body: String::new(),
            provider: None,
            model: None,
            thread: None,
        })?;

        assert!(response.contains("\"object\":\"list\""));
        assert!(response.contains("daemon-openai-chat"));
        Ok(())
    }

    #[test]
    fn daemon_once_models_endpoint_does_not_materialize_thread_history()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = super::daemon_once_result(DaemonCommand {
            once: true,
            method: "GET".to_owned(),
            endpoint: "/v1/models".to_owned(),
            request_id: "models".to_owned(),
            body: String::new(),
            provider: None,
            model: None,
            thread: Some("demo".to_owned()),
        })?;

        assert!(result.body.contains("\"object\":\"list\""));
        assert_eq!(result.thread, None);
        Ok(())
    }

    #[test]
    fn parser_rejects_non_abi_command() {
        assert_eq!(
            Command::parse([OsString::from("serve")]),
            Command::Unknown(super::UnknownCommand::new("serve"))
        );
    }

    #[test]
    fn parser_rejects_non_abi_runtime_command() {
        assert_eq!(
            Command::parse([OsString::from("run")]),
            Command::Unknown(super::UnknownCommand::new("run"))
        );
    }

    #[test]
    fn parser_rejects_development_runtime_shortcuts() {
        for command in ["dev", "watch", "hot-reload"] {
            assert_eq!(
                Command::parse([OsString::from(command)]),
                Command::Unknown(super::UnknownCommand::new(command))
            );
        }
    }

    #[test]
    fn parser_builds_single_user_mount_command() {
        let command = Command::parse([OsString::from("mount"), OsString::from("/mnt/cortex")]);

        assert_eq!(
            command,
            Command::Mount(MountCommand {
                mountpoint: PathBuf::from("/mnt/cortex"),
                multi_user: false,
            })
        );
    }

    #[test]
    fn parser_rejects_unknown_mount_argument() {
        assert_eq!(
            Command::parse([OsString::from("mount"), OsString::from("--watch")]),
            Command::Invalid(super::InvalidCommand::new(
                "unknown mount argument: --watch"
            ))
        );
    }

    #[test]
    fn parser_rejects_extra_mountpoint() {
        assert_eq!(
            Command::parse([
                OsString::from("mount"),
                OsString::from("/mnt/a"),
                OsString::from("/mnt/b"),
            ]),
            Command::Invalid(super::InvalidCommand::new(
                "unexpected extra mountpoint: /mnt/b"
            ))
        );
    }

    #[test]
    fn multi_user_mount_command_requests_allow_other_without_relaxing_hardening() {
        let command = MountCommand {
            mountpoint: PathBuf::from("/mnt/cortex"),
            multi_user: true,
        };
        let config = command.fuse_config();
        let security = config.options().security();

        assert_eq!(config.options().mode(), cortexfs::MountMode::MultiUser);
        assert!(security.allow_other());
        assert!(security.default_permissions());
        assert!(security.noexec());
        assert!(security.nodev());
        assert!(security.nosuid());
    }
}
