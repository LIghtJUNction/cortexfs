#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "binary sibling modules share bounded internal types"
)]
#![expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "wire structs expose fields only within this binary crate"
)]
mod client;
mod config;
mod project;
mod tool;
use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const BWRAP_PROGRAM: &str = "/usr/bin/bwrap";
const RUNTIME_ENV: [&str; 2] = ["CTX_AUTHORIZED_OBJECT", "CTX_RUN_ID"];

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            let _ignored = writeln!(
                io::stderr().lock(),
                "ctxmcp: {}",
                cortexfs::support::terminal::terminal_safe_text(&error.to_string())
            );
            ExitCode::from(69)
        }
    }
}

fn run() -> io::Result<ExitCode> {
    if std::process::id() != 1 {
        return run_as_pid_one();
    }
    if !environment_is_clean() {
        return run_clean();
    }
    if env::var_os("CTX_AUTHORIZED_OBJECT").is_some() {
        return tool::run();
    }
    let mut args = env::args_os().skip(1);
    let verb = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "usage: ctxmcp list|project"))?;
    let (mut config, mut server, mut runtime, mut out, mut policy) = (None, None, None, None, None);
    while let Some(flag) = args.next() {
        let flag = flag.into_string().map_err(|_value| {
            io::Error::new(io::ErrorKind::InvalidInput, "arguments must be UTF-8")
        })?;
        let value = args.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{flag} requires value"),
            )
        })?;
        match flag.as_str() {
            "--config" => config = Some(PathBuf::from(value)),
            "--server" => server = value.into_string().ok(),
            "--runtime-config" => runtime = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            "--policy-file" => policy = Some(PathBuf::from(value)),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown option {flag}"),
                ));
            }
        }
    }
    let config_path =
        config.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--config required"))?;
    let server_name =
        server.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--server required"))?;
    let server = config::read(&config_path, &server_name, None)?;
    let mut client = client::Client::start(&server)?;
    let tools = client.tools()?;
    if verb == "list" {
        let mut stdout = io::stdout().lock();
        for item in tools {
            writeln!(
                stdout,
                "{}.{}\t{}",
                cortexfs::support::terminal::terminal_safe_text(&server_name),
                cortexfs::support::terminal::terminal_safe_text(&item.name),
                cortexfs::support::terminal::terminal_safe_text(&item.description)
            )?;
        }
    } else if verb == "project" {
        let runtime = runtime.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "--runtime-config required")
        })?;
        let out =
            out.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--out required"))?;
        for path in project::write(
            &out,
            &env::current_exe()?,
            &runtime,
            &server_name,
            &tools,
            policy.as_deref(),
        )? {
            writeln!(
                io::stdout().lock(),
                "{}",
                cortexfs::support::terminal::terminal_safe_text(&path.to_string_lossy())
            )?;
        }
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "command must be list or project",
        ));
    }
    Ok(ExitCode::SUCCESS)
}

fn environment_is_clean() -> bool {
    env::vars_os().all(|(name, _)| {
        name.into_string()
            .is_ok_and(|name| RUNTIME_ENV.contains(&name.as_str()))
    })
}

fn run_clean() -> io::Result<ExitCode> {
    let mut command = Command::new("/proc/self/exe");
    command.args(env::args_os().skip(1));
    command.env_clear();
    for name in RUNTIME_ENV {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    Err(command.exec())
}

fn run_as_pid_one() -> io::Result<ExitCode> {
    let executable = env::current_exe()?;
    let status = pid_namespace_command(&executable, env::args_os().skip(1)).status()?;
    let code = status
        .code()
        .ok_or_else(|| io::Error::other("ctxmcp PID namespace terminated by signal"))?;
    let code = u8::try_from(code).map_err(|_range| {
        io::Error::new(io::ErrorKind::InvalidData, "process exit code out of range")
    })?;
    Ok(ExitCode::from(code))
}

fn pid_namespace_command(executable: &Path, args: impl IntoIterator<Item = OsString>) -> Command {
    let mut command = Command::new(BWRAP_PROGRAM);
    command
        .arg("--unshare-pid")
        .arg("--as-pid-1")
        .arg("--die-with-parent")
        .arg("--new-session")
        .arg("--bind")
        .arg("/")
        .arg("/")
        .arg("--proc")
        .arg("/proc")
        .arg("--dev-bind")
        .arg("/dev")
        .arg("/dev")
        .arg("--clearenv");

    for name in RUNTIME_ENV {
        if let Some(value) = env::var_os(name) {
            command.arg("--setenv").arg(name).arg(value);
        }
    }

    command
        .arg("--")
        .arg(executable)
        .args(args)
        .env_clear()
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    command
}
