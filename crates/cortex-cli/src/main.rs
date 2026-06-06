#![forbid(unsafe_code)]

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
            "init" => Self::Init(InitCommand),
            "daemon" => Self::Daemon(DaemonCommand),
            "mount" => Self::Mount(MountCommand::parse(arguments)),
            "status" => Self::Status(StatusCommand),
            unknown => Self::Unknown(UnknownCommand::new(unknown)),
        }
    }
}

/// Placeholder for `cortex init`.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct InitCommand;

/// Placeholder for `cortex daemon`.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct DaemonCommand;

/// Placeholder for `cortex mount`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MountCommand {
    mountpoint: PathBuf,
    multi_user: bool,
}

impl MountCommand {
    fn parse(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Self {
        let mut mountpoint = PathBuf::from("mnt/cortex");
        let mut multi_user = false;

        for argument in arguments {
            if argument == "--multi-user" {
                multi_user = true;
            } else {
                mountpoint = PathBuf::from(argument);
            }
        }

        Self {
            mountpoint,
            multi_user,
        }
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
            "status=ready\nabi=cortexfs.design.v0\nplatform=linux\nrecommended_mount=/ctx\nctx_home=/ctx/home/{uid}\ndefault_test_mount=tests/mounts/cortexfs\nlive_test_provider=provider-neutral\nlive_test_model=smollm2:135m\n"
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

/// Executes a parsed command.
///
/// # Errors
///
/// Returns [`CliError::NotImplemented`] for command placeholders and propagates
/// mount scaffolding errors.
pub fn run(command: Command) -> Result<(), CliError> {
    match command {
        Command::Init(_command) => Err(CliError::NotImplemented("init")),
        Command::Daemon(_command) => Err(CliError::NotImplemented("daemon")),
        Command::Mount(command) => {
            let config = command.fuse_config();
            cortexfs::mount(&config)?;
            Ok(())
        }
        Command::Status(_command) => print_output(&StatusCommand::render()),
        Command::Unknown(command) => Err(CliError::UnknownCommand(command.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, MountCommand, StatusCommand};
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
        assert!(status.contains("ctx_home=/ctx/home/"));
        assert!(status.contains("default_test_mount=tests/mounts/cortexfs\n"));
        assert!(status.contains("live_test_provider=provider-neutral\n"));
        assert!(status.contains("live_test_model=smollm2:135m\n"));
    }

    #[test]
    fn parses_effective_uid_from_proc_status() {
        let status = "Name:\tcortex\nUid:\t1000\t2000\t3000\t4000\n";

        assert_eq!(super::parse_effective_uid(status), Some(2000));
    }

    #[test]
    fn parser_rejects_unknown_command() {
        assert_eq!(
            Command::parse([OsString::from("reload")]),
            Command::Unknown(super::UnknownCommand::new("reload"))
        );
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
    fn multi_user_mount_command_requests_allow_other() {
        let command = MountCommand {
            mountpoint: PathBuf::from("/mnt/cortex"),
            multi_user: true,
        };
        let config = command.fuse_config();

        assert_eq!(config.options().mode(), cortexfs::MountMode::MultiUser);
        assert!(config.options().security().allow_other());
        assert!(config.options().security().default_permissions());
    }
}
