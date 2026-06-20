#![forbid(unsafe_code)]

use cortex_core::{ApiFormat, ProviderId, ThreadId};
use cortex_providers::{InMemoryProvider, ProviderResponse};
use cortex_store::{InMemoryStore, RequestId, Store, ThreadSnapshot};
use cortexd::{ExecutionPlane, LocalApiEndpoint, LocalApiRequest};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::str::FromStr as _;
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// Prepare the system mountpoint before systemd starts FUSE.
    MountPrep(MountPrepCommand),
    /// Manage the systemd-backed mount service.
    Service(ServiceCommand),
    /// Report daemon and mount status.
    Status(StatusCommand),
    /// Read the mounted filesystem ABI marker.
    Abi(MountReadCommand),
    /// Read the mounted filesystem implementation version.
    Version(MountReadCommand),
    /// Manage provider registry entries through the filesystem ABI.
    Provider(ProviderCommand),
    /// Inspect and manage the local aggregate API projection.
    Api(ApiCommand),
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
            "mount-prep" => match MountPrepCommand::parse(arguments) {
                Ok(command) => Self::MountPrep(command),
                Err(error) => Self::Invalid(error),
            },
            "start" => match parse_no_arguments(arguments, "start") {
                Ok(()) => Self::Service(ServiceCommand::start()),
                Err(error) => Self::Invalid(error),
            },
            "stop" => match parse_no_arguments(arguments, "stop") {
                Ok(()) => Self::Service(ServiceCommand::stop()),
                Err(error) => Self::Invalid(error),
            },
            "restart" => match parse_no_arguments(arguments, "restart") {
                Ok(()) => Self::Service(ServiceCommand::restart()),
                Err(error) => Self::Invalid(error),
            },
            "status" => match parse_no_arguments(arguments, "status") {
                Ok(()) => Self::Status(StatusCommand),
                Err(error) => Self::Invalid(error),
            },
            "abi" => match MountReadCommand::parse(arguments, "abi") {
                Ok(command) => Self::Abi(command),
                Err(error) => Self::Invalid(error),
            },
            "version" => match MountReadCommand::parse(arguments, "version") {
                Ok(command) => Self::Version(command),
                Err(error) => Self::Invalid(error),
            },
            "provider" => match ProviderCommand::parse(arguments) {
                Ok(command) => Self::Provider(command),
                Err(error) => Self::Invalid(error),
            },
            "api" => match ApiCommand::parse(arguments) {
                Ok(command) => Self::Api(command),
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

/// systemd service operation for the background mount.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ServiceAction {
    /// Enable and start the mount service.
    Start,
    /// Stop the mount service.
    Stop,
    /// Restart the mount service.
    Restart,
}

impl ServiceAction {
    const fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

/// Manage `cortexfs@<user>.service`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ServiceCommand {
    action: ServiceAction,
}

impl ServiceCommand {
    const fn start() -> Self {
        Self {
            action: ServiceAction::Start,
        }
    }

    const fn stop() -> Self {
        Self {
            action: ServiceAction::Stop,
        }
    }

    const fn restart() -> Self {
        Self {
            action: ServiceAction::Restart,
        }
    }
}

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
        let mut mountpoint = PathBuf::from("/ctx");
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

/// Internal mountpoint setup command used by the systemd unit.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MountPrepCommand {
    mountpoint: PathBuf,
    user: String,
    group: String,
}

impl MountPrepCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut arguments = arguments.into_iter();
        let Some(mountpoint) = arguments.next() else {
            return Err(InvalidCommand::new("missing mount-prep mountpoint"));
        };
        let Some(user) = arguments.next() else {
            return Err(InvalidCommand::new("missing mount-prep user"));
        };
        let Some(group) = arguments.next() else {
            return Err(InvalidCommand::new("missing mount-prep group"));
        };
        if let Some(extra) = arguments.next() {
            return Err(InvalidCommand::new(format!(
                "unexpected mount-prep argument: {}",
                extra.to_string_lossy()
            )));
        }
        Ok(Self {
            mountpoint: PathBuf::from(mountpoint),
            user: user.to_string_lossy().into_owned(),
            group: group.to_string_lossy().into_owned(),
        })
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MountReadCommand {
    mountpoint: PathBuf,
}

impl MountReadCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
        command: &str,
    ) -> Result<Self, InvalidCommand> {
        let mut mountpoint = PathBuf::from("/ctx");
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.to_string_lossy().as_ref() {
                "--mount" => {
                    mountpoint = PathBuf::from(parse_required_cli_value(
                        &mut arguments,
                        command,
                        "--mount",
                    )?);
                }
                unknown => {
                    return Err(InvalidCommand::new(format!(
                        "unknown {command} argument: {unknown}"
                    )));
                }
            }
        }
        Ok(Self { mountpoint })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ProviderCommand {
    List(ProviderListCommand),
    Add(ProviderAddCommand),
    Key(ProviderKeyCommand),
}

impl ProviderCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut arguments = arguments.into_iter();
        let Some(subcommand) = arguments.next() else {
            return Err(InvalidCommand::new("missing provider subcommand"));
        };
        match subcommand.to_string_lossy().as_ref() {
            "list" => ProviderListCommand::parse(arguments).map(Self::List),
            "add" => ProviderAddCommand::parse(arguments).map(Self::Add),
            "key" => ProviderKeyCommand::parse(arguments).map(Self::Key),
            unknown => Err(InvalidCommand::new(format!(
                "unknown provider subcommand: {unknown}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderListCommand {
    mountpoint: PathBuf,
}

impl ProviderListCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut mountpoint = PathBuf::from("/ctx");
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.to_string_lossy().as_ref() {
                "--mount" => {
                    mountpoint = PathBuf::from(parse_required_cli_value(
                        &mut arguments,
                        "provider list",
                        "--mount",
                    )?);
                }
                unknown => {
                    return Err(InvalidCommand::new(format!(
                        "unknown provider list argument: {unknown}"
                    )));
                }
            }
        }
        Ok(Self { mountpoint })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderAddCommand {
    mountpoint: PathBuf,
    id: String,
    family: String,
    name: Option<String>,
    formats: Vec<String>,
    base_url: String,
    default_model: String,
    priority: u32,
    enabled: bool,
}

impl ProviderAddCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut mountpoint = PathBuf::from("/ctx");
        let mut id = None;
        let mut family = "openai-compatible".to_owned();
        let mut name = None;
        let mut formats = Vec::new();
        let mut base_url = None;
        let mut default_model = None;
        let mut priority = 80;
        let mut enabled = true;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.to_string_lossy().as_ref() {
                "--mount" => {
                    mountpoint = PathBuf::from(parse_required_cli_value(
                        &mut arguments,
                        "provider add",
                        "--mount",
                    )?);
                }
                "--id" => {
                    id = Some(parse_required_cli_value(
                        &mut arguments,
                        "provider add",
                        "--id",
                    )?);
                }
                "--family" => {
                    family = parse_required_cli_value(&mut arguments, "provider add", "--family")?;
                }
                "--name" => {
                    name = Some(parse_required_cli_value(
                        &mut arguments,
                        "provider add",
                        "--name",
                    )?);
                }
                "--format" | "--protocol" => {
                    let format =
                        parse_required_cli_value(&mut arguments, "provider add", "--format")?;
                    ApiFormat::from_str(&format)
                        .map_err(|error| InvalidCommand::new(error.to_string()))?;
                    formats.push(format);
                }
                "--base-url" => {
                    base_url = Some(parse_required_cli_value(
                        &mut arguments,
                        "provider add",
                        "--base-url",
                    )?);
                }
                "--model" | "--default-model" => {
                    default_model = Some(parse_required_cli_value(
                        &mut arguments,
                        "provider add",
                        "--model",
                    )?);
                }
                "--priority" => {
                    let value =
                        parse_required_cli_value(&mut arguments, "provider add", "--priority")?;
                    priority = value.parse().map_err(|_error| {
                        InvalidCommand::new("provider add --priority must be an integer")
                    })?;
                }
                "--disabled" => enabled = false,
                unknown => {
                    return Err(InvalidCommand::new(format!(
                        "unknown provider add argument: {unknown}"
                    )));
                }
            }
        }
        let id = id.ok_or_else(|| InvalidCommand::new("missing provider add argument: --id"))?;
        ProviderId::new(id.clone()).map_err(|error| InvalidCommand::new(error.to_string()))?;
        let base_url = base_url
            .ok_or_else(|| InvalidCommand::new("missing provider add argument: --base-url"))?;
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(InvalidCommand::new(
                "provider add --base-url must start with http:// or https://",
            ));
        }
        let default_model = default_model
            .ok_or_else(|| InvalidCommand::new("missing provider add argument: --model"))?;
        cortex_core::ModelId::new(default_model.clone())
            .map_err(|error| InvalidCommand::new(error.to_string()))?;
        if formats.is_empty() {
            formats.push("openai.chat".to_owned());
            formats.push("openai.responses".to_owned());
        }
        formats.sort();
        formats.dedup();
        Ok(Self {
            mountpoint,
            id,
            family,
            name,
            formats,
            base_url,
            default_model,
            priority,
            enabled,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ProviderKeyCommand {
    Refresh(ProviderKeyRefreshCommand),
}

impl ProviderKeyCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut arguments = arguments.into_iter();
        let Some(subcommand) = arguments.next() else {
            return Err(InvalidCommand::new("missing provider key subcommand"));
        };
        match subcommand.to_string_lossy().as_ref() {
            "refresh" | "import" => ProviderKeyRefreshCommand::parse(arguments).map(Self::Refresh),
            unknown => Err(InvalidCommand::new(format!(
                "unknown provider key subcommand: {unknown}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderKeyRefreshCommand {
    mountpoint: PathBuf,
    provider: String,
    source: SecretInputSource,
}

impl ProviderKeyRefreshCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut mountpoint = PathBuf::from("/ctx");
        let mut provider = None;
        let mut source = SecretInputSource::Stdin;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.to_string_lossy().as_ref() {
                "--mount" => {
                    mountpoint = PathBuf::from(parse_required_cli_value(
                        &mut arguments,
                        "provider key refresh",
                        "--mount",
                    )?);
                }
                "--provider" => {
                    provider = Some(parse_required_cli_value(
                        &mut arguments,
                        "provider key refresh",
                        "--provider",
                    )?);
                }
                "--stdin" => source = SecretInputSource::Stdin,
                "--key-file" => {
                    source = SecretInputSource::File(PathBuf::from(parse_required_cli_value(
                        &mut arguments,
                        "provider key refresh",
                        "--key-file",
                    )?));
                }
                unknown => {
                    return Err(InvalidCommand::new(format!(
                        "unknown provider key refresh argument: {unknown}"
                    )));
                }
            }
        }
        let provider = provider.ok_or_else(|| {
            InvalidCommand::new("missing provider key refresh argument: --provider")
        })?;
        ProviderId::new(provider.clone())
            .map_err(|error| InvalidCommand::new(error.to_string()))?;
        Ok(Self {
            mountpoint,
            provider,
            source,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SecretInputSource {
    Stdin,
    File(PathBuf),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ApiCommand {
    Status(ApiStatusCommand),
    Enable(ApiToggleCommand),
    Disable(ApiToggleCommand),
    Key(ApiKeyCommand),
}

impl ApiCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut arguments = arguments.into_iter();
        let Some(subcommand) = arguments.next() else {
            return Err(InvalidCommand::new("missing api subcommand"));
        };
        match subcommand.to_string_lossy().as_ref() {
            "status" => ApiStatusCommand::parse(arguments).map(Self::Status),
            "enable" => ApiToggleCommand::parse(arguments, true).map(Self::Enable),
            "disable" => ApiToggleCommand::parse(arguments, false).map(Self::Disable),
            "key" => ApiKeyCommand::parse(arguments).map(Self::Key),
            unknown => Err(InvalidCommand::new(format!(
                "unknown api subcommand: {unknown}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ApiStatusCommand {
    mountpoint: PathBuf,
}

impl ApiStatusCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mountpoint = parse_optional_mount(arguments, "api status")?;
        Ok(Self { mountpoint })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ApiToggleCommand {
    mountpoint: PathBuf,
    enabled: bool,
}

impl ApiToggleCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
        enabled: bool,
    ) -> Result<Self, InvalidCommand> {
        let mountpoint = parse_optional_mount(
            arguments,
            if enabled { "api enable" } else { "api disable" },
        )?;
        Ok(Self {
            mountpoint,
            enabled,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ApiKeyCommand {
    Refresh(ApiKeyRefreshCommand),
}

impl ApiKeyCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut arguments = arguments.into_iter();
        let Some(subcommand) = arguments.next() else {
            return Err(InvalidCommand::new("missing api key subcommand"));
        };
        match subcommand.to_string_lossy().as_ref() {
            "refresh" => ApiKeyRefreshCommand::parse(arguments).map(Self::Refresh),
            unknown => Err(InvalidCommand::new(format!(
                "unknown api key subcommand: {unknown}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ApiKeyRefreshCommand {
    mountpoint: PathBuf,
    source: SecretInputSource,
}

impl ApiKeyRefreshCommand {
    fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, InvalidCommand> {
        let mut mountpoint = PathBuf::from("/ctx");
        let mut source = SecretInputSource::Stdin;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.to_string_lossy().as_ref() {
                "--mount" => {
                    mountpoint = PathBuf::from(parse_required_cli_value(
                        &mut arguments,
                        "api key refresh",
                        "--mount",
                    )?);
                }
                "--stdin" => source = SecretInputSource::Stdin,
                "--key-file" => {
                    source = SecretInputSource::File(PathBuf::from(parse_required_cli_value(
                        &mut arguments,
                        "api key refresh",
                        "--key-file",
                    )?));
                }
                unknown => {
                    return Err(InvalidCommand::new(format!(
                        "unknown api key refresh argument: {unknown}"
                    )));
                }
            }
        }
        Ok(Self { mountpoint, source })
    }
}

fn parse_optional_mount(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
    command: &str,
) -> Result<PathBuf, InvalidCommand> {
    let mut mountpoint = PathBuf::from("/ctx");
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.to_string_lossy().as_ref() {
            "--mount" => {
                mountpoint = PathBuf::from(parse_required_cli_value(
                    &mut arguments,
                    command,
                    "--mount",
                )?);
            }
            unknown => {
                return Err(InvalidCommand::new(format!(
                    "unknown {command} argument: {unknown}"
                )));
            }
        }
    }
    Ok(mountpoint)
}

fn parse_required_cli_value(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    command: &str,
    flag: &str,
) -> Result<String, InvalidCommand> {
    let Some(value) = arguments.next() else {
        return Err(InvalidCommand::new(format!(
            "missing {command} argument value: {flag}"
        )));
    };
    if value.to_string_lossy().starts_with("--") {
        return Err(InvalidCommand::new(format!(
            "missing {command} argument value: {flag}"
        )));
    }
    Ok(value.to_string_lossy().into_owned())
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
    /// systemd service management failed.
    Service(String),
    /// system mountpoint preparation failed.
    MountPrep(String),
    /// Mounted filesystem ABI did not expose the requested control file.
    UnsupportedAbi(String),
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
            Self::Service(ref message)
            | Self::MountPrep(ref message)
            | Self::UnsupportedAbi(ref message) => write!(f, "{message}"),
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
        Command::MountPrep(command) => run_mount_prep(&command),
        Command::Service(command) => run_service(command),
        Command::Status(_command) => print_output(&StatusCommand::render()),
        Command::Abi(command) => print_mount_file(&command.mountpoint, &["control", "abi"]),
        Command::Version(command) => print_mount_file(&command.mountpoint, &["control", "version"]),
        Command::Provider(command) => run_provider(command),
        Command::Api(command) => run_api(command),
        Command::Invalid(command) => Err(CliError::InvalidCommand(command.message)),
        Command::Unknown(command) => Err(CliError::UnknownCommand(command.name)),
    }
}

fn run_provider(command: ProviderCommand) -> Result<(), CliError> {
    match command {
        ProviderCommand::List(command) => print_output(&render_provider_list(&command.mountpoint)?),
        ProviderCommand::Add(command) => run_provider_add(command),
        ProviderCommand::Key(command) => match command {
            ProviderKeyCommand::Refresh(command) => run_provider_key_refresh(&command),
        },
    }
}

fn run_api(command: ApiCommand) -> Result<(), CliError> {
    match command {
        ApiCommand::Status(command) => print_output(&render_api_status(&command.mountpoint)?),
        ApiCommand::Enable(command) | ApiCommand::Disable(command) => run_api_toggle(&command),
        ApiCommand::Key(command) => match command {
            ApiKeyCommand::Refresh(command) => run_api_key_refresh(command),
        },
    }
}

fn print_mount_file(mountpoint: &Path, components: &[&str]) -> Result<(), CliError> {
    let content = read_mount_file(mountpoint, components)?;
    print_output(&content)
}

fn read_mount_file(mountpoint: &Path, components: &[&str]) -> Result<String, CliError> {
    std::fs::read_to_string(join_components(mountpoint, components)).map_err(CliError::Io)
}

fn write_mount_file(mountpoint: &Path, components: &[&str], content: &str) -> Result<(), CliError> {
    std::fs::write(join_components(mountpoint, components), content).map_err(CliError::Io)
}

fn join_components(mountpoint: &Path, components: &[&str]) -> PathBuf {
    let mut path = mountpoint.to_path_buf();
    for component in components {
        path.push(component);
    }
    path
}

fn render_provider_list(mountpoint: &Path) -> Result<String, CliError> {
    let providers = read_mount_file(mountpoint, &["provider", "list"])?;
    let mut output = String::from("id\tenabled\tsecret\tformats\tbase_url\n");
    for provider in providers
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let enabled = read_mount_file(mountpoint, &["provider", provider, "enabled", "effective"])
            .unwrap_or_else(|_error| "\n".to_owned())
            .trim()
            .to_owned();
        let secret = read_mount_file(mountpoint, &["provider", provider, "secrets", "status"])
            .unwrap_or_else(|_error| "\n".to_owned())
            .trim()
            .to_owned();
        let formats = read_mount_file(mountpoint, &["provider", provider, "format"])
            .unwrap_or_else(|_error| "\n".to_owned())
            .lines()
            .collect::<Vec<_>>()
            .join(",");
        let base_url = read_mount_file(mountpoint, &["provider", provider, "url", "effective"])
            .unwrap_or_else(|_error| "\n".to_owned())
            .trim()
            .to_owned();
        output.push_str(provider);
        output.push('\t');
        output.push_str(&enabled);
        output.push('\t');
        output.push_str(&secret);
        output.push('\t');
        output.push_str(&formats);
        output.push('\t');
        output.push_str(&base_url);
        output.push('\n');
    }
    Ok(output)
}

fn run_provider_add(command: ProviderAddCommand) -> Result<(), CliError> {
    let provider_id = command.id.clone();
    let body = serde_json::json!({
        "op": "upsert",
        "id": provider_id,
        "family": command.family,
        "name": command.name.unwrap_or_else(|| command.id.clone()),
        "formats": command.formats,
        "base_url": command.base_url,
        "default_model": command.default_model,
        "priority": command.priority,
        "enabled": command.enabled,
    })
    .to_string();
    let response = submit_json_request(
        &command.mountpoint,
        &["provider", "inbox"],
        &["provider", "outbox"],
        &command.id,
        &body,
    )?;
    print_output(&response)?;
    print_output("\n")
}

fn run_provider_key_refresh(command: &ProviderKeyRefreshCommand) -> Result<(), CliError> {
    let value = read_secret_input(&command.source)?;
    let body = serde_json::json!({
        "op": "import",
        "kind": "bearer",
        "value": value.trim_end_matches(['\r', '\n']),
    })
    .to_string();
    let response = submit_json_request(
        &command.mountpoint,
        &["provider", command.provider.as_str(), "secrets", "inbox"],
        &["provider", command.provider.as_str(), "secrets", "outbox"],
        "api-key",
        &body,
    )?;
    print_output(&response)?;
    print_output("\n")
}

fn submit_json_request(
    mountpoint: &Path,
    inbox_components: &[&str],
    outbox_components: &[&str],
    request_id: &str,
    body: &str,
) -> Result<String, CliError> {
    let inbox = join_components(mountpoint, inbox_components);
    let temp_name = format!("{request_id}.{}.tmp", request_nonce());
    let temp_path = inbox.join(&temp_name);
    std::fs::write(&temp_path, body).map_err(CliError::Io)?;
    let request_name = format!("{request_id}.req.json");
    let request_path = inbox.join(request_name);
    std::fs::rename(&temp_path, &request_path).map_err(CliError::Io)?;
    let outbox = join_components(mountpoint, outbox_components);
    let response_path = outbox.join(format!("{request_id}.resp.json"));
    match std::fs::read_to_string(&response_path) {
        Ok(response) => return Ok(response),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(CliError::Io(error));
        }
        Err(_error) => {}
    }
    let error_path = outbox.join(format!("{request_id}.error"));
    match std::fs::read_to_string(&error_path) {
        Ok(error) => Err(CliError::UnsupportedAbi(format!(
            "filesystem request failed: {}",
            error.trim()
        ))),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn request_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

fn read_secret_input(source: &SecretInputSource) -> Result<String, CliError> {
    let mut value = String::new();
    match *source {
        SecretInputSource::Stdin => {
            std::io::stdin()
                .read_to_string(&mut value)
                .map_err(CliError::Io)?;
        }
        SecretInputSource::File(ref path) => {
            value = std::fs::read_to_string(path).map_err(CliError::Io)?;
        }
    }
    if value.trim().is_empty() {
        return Err(CliError::InvalidCommand(
            "secret input must not be empty".to_owned(),
        ));
    }
    Ok(value)
}

fn render_api_status(mountpoint: &Path) -> Result<String, CliError> {
    let home = resolve_ctx_home(mountpoint);
    let api = home.join("api");
    let mut output = String::new();
    append_optional_file(&mut output, "status", &api.join("status"))?;
    append_optional_file(&mut output, "abi", &api.join("abi"))?;
    append_optional_file(&mut output, "endpoints", &api.join("endpoints"))?;
    append_optional_file(&mut output, "http_status", &api.join("http/status"))?;
    append_optional_file(&mut output, "http_listen", &api.join("http/listen"))?;
    append_optional_file(&mut output, "http_base_url", &api.join("http/localurl"))?;
    append_optional_file(&mut output, "unix_status", &api.join("unix/status"))?;
    append_optional_file(&mut output, "unix_path", &api.join("unix/path"))?;
    Ok(output)
}

fn append_optional_file(output: &mut String, key: &str, path: &Path) -> Result<(), CliError> {
    match std::fs::read_to_string(path) {
        Ok(value) => {
            let value = value.trim();
            output.push_str(key);
            output.push('=');
            output.push_str(value);
            output.push('\n');
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn resolve_ctx_home(mountpoint: &Path) -> PathBuf {
    let current = mountpoint.join("home").join(current_uid().to_string());
    if current.exists() {
        return current;
    }
    mountpoint.join("home").join("1000")
}

fn run_api_toggle(command: &ApiToggleCommand) -> Result<(), CliError> {
    let home = resolve_ctx_home(&command.mountpoint);
    let enabled = home.join("api/http/enabled");
    if !enabled.exists() {
        return Err(CliError::UnsupportedAbi(
            "local aggregate API enable/disable is not exposed by the current filesystem ABI"
                .to_owned(),
        ));
    }
    let value = if command.enabled { "1\n" } else { "0\n" };
    write_mount_file(&home, &["api", "http", "enabled"], value)
}

fn run_api_key_refresh(_command: ApiKeyRefreshCommand) -> Result<(), CliError> {
    Err(CliError::UnsupportedAbi(
        "local aggregate API ingress key refresh is not exposed by the current filesystem ABI"
            .to_owned(),
    ))
}

fn run_mount_prep(command: &MountPrepCommand) -> Result<(), CliError> {
    ensure_fuse_device()?;
    clean_mountpoint(&command.mountpoint);
    let status = ProcessCommand::new("/usr/bin/install")
        .args([
            "-d",
            "-o",
            command.user.as_str(),
            "-g",
            command.group.as_str(),
            "-m",
            "0755",
        ])
        .arg(&command.mountpoint)
        .status()
        .map_err(|error| {
            CliError::MountPrep(format!(
                "failed to prepare {}: {error}",
                command.mountpoint.display()
            ))
        })?;
    if status.success() {
        return Ok(());
    }
    Err(CliError::MountPrep(format!(
        "failed to prepare {} for {}:{}: {status}",
        command.mountpoint.display(),
        command.user,
        command.group
    )))
}

fn ensure_fuse_device() -> Result<(), CliError> {
    if Path::new("/dev/fuse").exists() {
        return Ok(());
    }
    let status = quiet_process("/usr/bin/modprobe")
        .arg("fuse")
        .status()
        .map_err(|error| CliError::MountPrep(format!("failed to load fuse module: {error}")))?;
    if status.success() || Path::new("/dev/fuse").exists() {
        return Ok(());
    }
    Err(CliError::MountPrep(format!(
        "failed to load fuse module: {status}"
    )))
}

fn clean_mountpoint(mountpoint: &Path) {
    let _status = quiet_process("/usr/bin/fusermount3")
        .args(["-uz"])
        .arg(mountpoint)
        .status();
    let _status = quiet_process("/usr/bin/umount")
        .args(["-l"])
        .arg(mountpoint)
        .status();
}

fn quiet_process(program: &str) -> ProcessCommand {
    let mut command = ProcessCommand::new(program);
    command.stdout(Stdio::null()).stderr(Stdio::null());
    command
}

fn run_service(command: ServiceCommand) -> Result<(), CliError> {
    let user = service_user()?;
    let unit = format!("cortexfs@{user}.service");
    let manager = service_manager_name();
    let args: Vec<&str> = match command.action {
        ServiceAction::Start => vec!["enable", "--now", unit.as_str()],
        ServiceAction::Stop => vec!["stop", unit.as_str()],
        ServiceAction::Restart => vec!["restart", unit.as_str()],
    };
    let mut process = service_manager();
    let status = process
        .args(args)
        .status()
        .map_err(|error| {
            CliError::Service(format!(
                "failed to run {manager} for {unit}: {error}. cortex {} manages a system service and needs admin rights",
                command.action.name()
            ))
        })?;
    if status.success() {
        return Ok(());
    }
    Err(CliError::Service(format!(
        "{manager} failed for {unit}: {status}. authorize the admin prompt or run sudo cortex {}",
        command.action.name()
    )))
}

fn service_user() -> Result<String, CliError> {
    if current_uid() == 0
        && let Ok(user) = std::env::var("SUDO_USER")
        && !user.is_empty()
        && user != "root"
    {
        return Ok(user);
    }
    std::env::var("USER")
        .ok()
        .filter(|user| !user.is_empty())
        .ok_or_else(|| CliError::Service("USER is not set for cortexfs service".to_owned()))
}

fn service_manager() -> ProcessCommand {
    if current_uid() == 0 {
        return ProcessCommand::new("systemctl");
    }
    let mut command = ProcessCommand::new("sudo");
    command.arg("--").arg("systemctl");
    command
}

fn service_manager_name() -> &'static str {
    if current_uid() == 0 {
        "systemctl"
    } else {
        "sudo systemctl"
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
    use super::{
        ApiCommand, Command, DaemonCommand, MountCommand, MountPrepCommand, ProviderCommand,
        ServiceAction, ServiceCommand, StatusCommand,
    };
    use cortex_core::MessageRole;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn parser_accepts_abi_and_version_mount_overrides() {
        assert_eq!(
            Command::parse([
                OsString::from("abi"),
                OsString::from("--mount"),
                OsString::from("/mnt/ctx"),
            ]),
            Command::Abi(super::MountReadCommand {
                mountpoint: PathBuf::from("/mnt/ctx"),
            })
        );
        assert_eq!(
            Command::parse([
                OsString::from("version"),
                OsString::from("--mount"),
                OsString::from("/mnt/ctx"),
            ]),
            Command::Version(super::MountReadCommand {
                mountpoint: PathBuf::from("/mnt/ctx"),
            })
        );
    }

    #[test]
    fn parser_accepts_provider_add_with_formats() -> Result<(), Box<dyn std::error::Error>> {
        let command = Command::parse([
            OsString::from("provider"),
            OsString::from("add"),
            OsString::from("--id"),
            OsString::from("relay-a"),
            OsString::from("--base-url"),
            OsString::from("https://relay.example/v1"),
            OsString::from("--model"),
            OsString::from("gpt-5.4-mini"),
            OsString::from("--format"),
            OsString::from("openai.responses"),
            OsString::from("--format"),
            OsString::from("openai.chat"),
            OsString::from("--priority"),
            OsString::from("60"),
        ]);

        let Command::Provider(ProviderCommand::Add(command)) = command else {
            return Err("expected provider add command".into());
        };
        assert_eq!(command.id, "relay-a");
        assert_eq!(command.base_url, "https://relay.example/v1");
        assert_eq!(command.default_model, "gpt-5.4-mini");
        assert_eq!(command.formats, ["openai.chat", "openai.responses"]);
        assert_eq!(command.priority, 60);
        Ok(())
    }

    #[test]
    fn parser_accepts_provider_key_refresh_from_stdin() -> Result<(), Box<dyn std::error::Error>> {
        let command = Command::parse([
            OsString::from("provider"),
            OsString::from("key"),
            OsString::from("refresh"),
            OsString::from("--provider"),
            OsString::from("relay-a"),
            OsString::from("--stdin"),
        ]);

        let Command::Provider(ProviderCommand::Key(super::ProviderKeyCommand::Refresh(command))) =
            command
        else {
            return Err("expected provider key refresh command".into());
        };
        assert_eq!(command.provider, "relay-a");
        assert_eq!(command.source, super::SecretInputSource::Stdin);
        Ok(())
    }

    #[test]
    fn parser_accepts_api_status_and_toggle_commands() {
        assert_eq!(
            Command::parse([OsString::from("api"), OsString::from("status")]),
            Command::Api(ApiCommand::Status(super::ApiStatusCommand {
                mountpoint: PathBuf::from("/ctx"),
            }))
        );
        assert_eq!(
            Command::parse([OsString::from("api"), OsString::from("enable")]),
            Command::Api(ApiCommand::Enable(super::ApiToggleCommand {
                mountpoint: PathBuf::from("/ctx"),
                enabled: true,
            }))
        );
        assert_eq!(
            Command::parse([OsString::from("api"), OsString::from("disable")]),
            Command::Api(ApiCommand::Disable(super::ApiToggleCommand {
                mountpoint: PathBuf::from("/ctx"),
                enabled: false,
            }))
        );
    }

    #[test]
    fn provider_list_renders_filesystem_projection() -> Result<(), Box<dyn std::error::Error>> {
        let dir = temp_cli_tree("provider-list")?;
        write_file(&dir, &["provider", "list"], "relay-a\n")?;
        write_file(
            &dir,
            &["provider", "relay-a", "enabled", "effective"],
            "1\n",
        )?;
        write_file(
            &dir,
            &["provider", "relay-a", "secrets", "status"],
            "configured\n",
        )?;
        write_file(
            &dir,
            &["provider", "relay-a", "format"],
            "openai.chat\nopenai.responses\n",
        )?;
        write_file(
            &dir,
            &["provider", "relay-a", "url", "effective"],
            "https://relay.example/v1\n",
        )?;

        let output = super::render_provider_list(&dir)?;

        assert!(output.contains("id\tenabled\tsecret\tformats\tbase_url\n"));
        assert!(output.contains(
            "relay-a\t1\tconfigured\topenai.chat,openai.responses\thttps://relay.example/v1\n"
        ));
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn api_status_renders_current_user_projection() -> Result<(), Box<dyn std::error::Error>> {
        let dir = temp_cli_tree("api-status")?;
        let uid = super::current_uid().to_string();
        write_file(
            &dir,
            &["home", uid.as_str(), "api", "status"],
            "configured\n",
        )?;
        write_file(
            &dir,
            &["home", uid.as_str(), "api", "abi"],
            "cortex.local_api.v0\n",
        )?;
        write_file(
            &dir,
            &["home", uid.as_str(), "api", "http", "status"],
            "need-daemon\n",
        )?;
        write_file(
            &dir,
            &["home", uid.as_str(), "api", "http", "localurl"],
            "http://127.0.0.1:6185/v1\n",
        )?;

        let output = super::render_api_status(&dir)?;

        assert!(output.contains("status=configured\n"));
        assert!(output.contains("abi=cortex.local_api.v0\n"));
        assert!(output.contains("http_status=need-daemon\n"));
        assert!(output.contains("http_base_url=http://127.0.0.1:6185/v1\n"));
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    fn temp_cli_tree(name: &str) -> std::io::Result<PathBuf> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let dir =
            std::env::temp_dir().join(format!("cortex-cli-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn write_file(root: &Path, components: &[&str], content: &str) -> std::io::Result<()> {
        let mut path = root.to_path_buf();
        for component in components {
            path.push(component);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
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
    fn parser_accepts_systemd_service_commands() {
        assert_eq!(
            Command::parse([OsString::from("start")]),
            Command::Service(ServiceCommand {
                action: ServiceAction::Start,
            })
        );
        assert_eq!(
            Command::parse([OsString::from("stop")]),
            Command::Service(ServiceCommand {
                action: ServiceAction::Stop,
            })
        );
        assert_eq!(
            Command::parse([OsString::from("restart")]),
            Command::Service(ServiceCommand {
                action: ServiceAction::Restart,
            })
        );
    }

    #[test]
    fn parser_accepts_internal_mount_prep_command() {
        assert_eq!(
            Command::parse([
                OsString::from("mount-prep"),
                OsString::from("/ctx"),
                OsString::from("alice"),
                OsString::from("users"),
            ]),
            Command::MountPrep(MountPrepCommand {
                mountpoint: PathBuf::from("/ctx"),
                user: "alice".to_owned(),
                group: "users".to_owned(),
            })
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
