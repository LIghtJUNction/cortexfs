use std::process::ExitCode;

use listenfd::ListenFd;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = cortexfs::cli::stderr::write_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut listenfd = ListenFd::from_env();
    let listener = listenfd
        .take_unix_listener(0)
        .map_err(|error| format!("invalid systemd Unix listener: {error}"))?
        .ok_or_else(|| {
            "missing systemd Unix listener; start via systemd socket activation".to_owned()
        })?;
    cortexfs::runtime::terminal::broker::server::run(&listener)
        .map_err(|error| format!("broker server failed: {error}"))
}
