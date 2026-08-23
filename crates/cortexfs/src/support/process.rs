use crate::abi::constants::DEFAULT_SANDBOX_TMPFS_BYTES;
use std::{
    io::Read,
    process::{Child, Output},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Ordered bubblewrap isolation and process mounts after namespace flags.
///
/// Isolation flags (`--as-pid-1`, `--new-session`, `--cap-drop ALL`, hostname)
/// apply to every `CortexFS` sandbox so agents cannot inherit host session
/// leadership or residual capabilities.
#[must_use]
pub fn bwrap_process_setup_args() -> Vec<String> {
    vec![
        "--as-pid-1".to_owned(),
        "--new-session".to_owned(),
        "--cap-drop".to_owned(),
        "ALL".to_owned(),
        "--unshare-uts".to_owned(),
        "--hostname".to_owned(),
        "cortexfs".to_owned(),
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--size".to_owned(),
        DEFAULT_SANDBOX_TMPFS_BYTES.to_string(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--dir".to_owned(),
        "/run".to_owned(),
    ]
}

/// Ordered bubblewrap mounts and links for the base system layout.
const BWRAP_SYSTEM_LAYOUT_ARGS: [&str; 16] = [
    "--dir",
    "/home",
    "--ro-bind",
    "/usr",
    "/usr",
    "--ro-bind",
    "/etc",
    "/etc",
    "--tmpfs",
    "/etc/profile.d",
    "--symlink",
    "usr/bin",
    "/bin",
    "--symlink",
    "usr/lib",
    "/lib",
];

/// Base system layout plus a host-accurate `/lib64`.
///
/// A `usr/lib` symlink works on Arch, but Ubuntu keeps the dynamic loader under
/// `/usr/lib/x86_64-linux-gnu`, so the real `/lib64` must be bound when present.
pub fn bwrap_system_layout_args() -> Vec<String> {
    let mut args: Vec<String> = BWRAP_SYSTEM_LAYOUT_ARGS.map(str::to_owned).into();
    if std::path::Path::new("/lib64").exists() {
        args.extend(["--ro-bind", "/lib64", "/lib64"].map(str::to_owned));
    } else {
        args.extend(["--symlink", "usr/lib", "/lib64"].map(str::to_owned));
    }
    args
}

pub(crate) fn read_limited_bytes(mut reader: impl Read, limit: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let remaining = limit.saturating_sub(output.len());
        let kept = read.min(remaining);
        if let Some(chunk) = buffer.get(..kept) {
            output.extend_from_slice(chunk);
        }
        if output.len() >= limit {
            break;
        }
    }
    output
}

/// Lossy UTF-8 decode of a limited byte read.
///
/// Prefer this when the caller only needs text; use `read_limited_bytes` for
/// binary length checks before decoding.
#[must_use]
pub fn read_limited_text(reader: impl Read, limit: usize) -> String {
    String::from_utf8_lossy(&read_limited_bytes(reader, limit)).into_owned()
}

pub(crate) fn terminate_process_group(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        signal_process_group(pid, nix::sys::signal::Signal::SIGTERM);
        for _attempt in 0..5 {
            let _ignored = child.try_wait();
            thread::sleep(Duration::from_millis(50));
        }
        signal_process_group(pid, nix::sys::signal::Signal::SIGKILL);
    }
    let _ignored = child.kill();
}

pub(crate) fn signal_process_group(pid: i32, signal: nix::sys::signal::Signal) {
    let _ignored = nix::sys::signal::kill(nix::unistd::Pid::from_raw(-pid), signal);
}

/// Options for [`wait_capped_child_output`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct CappedOutputWait {
    /// Hard cap for each captured stream (stdout, and stderr when enabled).
    pub max_output_bytes: usize,
    /// Wall-clock limit for the child to exit.
    pub timeout: Duration,
    /// Capture stderr (requires a piped stderr handle).
    pub capture_stderr: bool,
    /// When set, unfinished stream readers must finish within this budget after exit.
    pub drain_timeout: Option<Duration>,
    /// After a normal wait, still SIGTERM the process group (orphan cleanup).
    pub terminate_group_after_exit: bool,
}

/// Failure while waiting for a capped child process.
#[derive(Debug)]
pub(crate) enum CappedOutputError {
    Wait(std::io::Error),
    ExceededLimit,
    TimedOut,
    Cancelled,
    DrainTimedOut,
}

/// Wait for `child` while draining piped stdout/(optional) stderr with a byte cap.
///
/// Callers must have already set `process_group(0)` and taken ownership of the pipes
/// only through this helper (stdout always; stderr when [`CappedOutputWait::capture_stderr`]).
pub(crate) fn wait_capped_child_output(
    child: &mut Child,
    options: CappedOutputWait,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Output, CappedOutputError> {
    let limit = options.max_output_bytes;
    let read_limit = limit.saturating_add(1);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CappedOutputError::Wait(std::io::Error::other("missing stdout pipe")))?;
    let mut stdout_reader: Option<JoinHandle<Vec<u8>>> = Some(thread::spawn(move || {
        read_limited_bytes(stdout, read_limit)
    }));
    let mut stderr_reader: Option<JoinHandle<Vec<u8>>> = if options.capture_stderr {
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CappedOutputError::Wait(std::io::Error::other("missing stderr pipe")))?;
        Some(thread::spawn(move || {
            read_limited_bytes(stderr, read_limit)
        }))
    } else {
        None
    };
    let mut stdout_buf = None;
    let mut stderr_buf = None;
    let deadline = Instant::now() + options.timeout;
    let status = loop {
        if take_finished_if_ready(&mut stdout_reader, &mut stdout_buf, limit) {
            abort_child(child, &mut stdout_reader, &mut stderr_reader);
            return Err(CappedOutputError::ExceededLimit);
        }
        if options.capture_stderr
            && take_finished_if_ready(&mut stderr_reader, &mut stderr_buf, limit)
        {
            abort_child(child, &mut stdout_reader, &mut stderr_reader);
            return Err(CappedOutputError::ExceededLimit);
        }
        if let Some(status) = child.try_wait().map_err(CappedOutputError::Wait)? {
            break status;
        }
        if Instant::now() >= deadline {
            abort_child(child, &mut stdout_reader, &mut stderr_reader);
            return Err(CappedOutputError::TimedOut);
        }
        if cancelled() {
            abort_child(child, &mut stdout_reader, &mut stderr_reader);
            return Err(CappedOutputError::Cancelled);
        }
        thread::sleep(Duration::from_millis(50));
    };
    if options.terminate_group_after_exit {
        terminate_process_group(child);
    }
    let stdout = join_reader(stdout_reader.take(), stdout_buf, options.drain_timeout)?;
    let stderr = if options.capture_stderr {
        join_reader(stderr_reader.take(), stderr_buf, options.drain_timeout)?
    } else {
        Vec::new()
    };
    if stdout.len() > limit || stderr.len() > limit {
        return Err(CappedOutputError::ExceededLimit);
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Returns `true` when a finished reader exceeded the cap (caller should abort).
fn take_finished_if_ready(
    reader: &mut Option<JoinHandle<Vec<u8>>>,
    buf: &mut Option<Vec<u8>>,
    limit: usize,
) -> bool {
    if buf.is_some() || !reader.as_ref().is_some_and(JoinHandle::is_finished) {
        return false;
    }
    let output = reader
        .take()
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    if output.len() > limit {
        return true;
    }
    *buf = Some(output);
    false
}

fn abort_child(
    child: &mut Child,
    stdout_reader: &mut Option<JoinHandle<Vec<u8>>>,
    stderr_reader: &mut Option<JoinHandle<Vec<u8>>>,
) {
    terminate_process_group(child);
    let _ignored = child.wait();
    drop(stdout_reader.take());
    drop(stderr_reader.take());
}

fn join_reader(
    reader: Option<JoinHandle<Vec<u8>>>,
    already: Option<Vec<u8>>,
    drain_timeout: Option<Duration>,
) -> Result<Vec<u8>, CappedOutputError> {
    if let Some(bytes) = already {
        return Ok(bytes);
    }
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    if let Some(timeout) = drain_timeout {
        let deadline = Instant::now() + timeout;
        while !reader.is_finished() {
            if Instant::now() >= deadline {
                return Err(CappedOutputError::DrainTimedOut);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(reader.join().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    #[test]
    fn read_limited_text_caps_and_decodes() {
        let input = b"hello world and more";
        let text = read_limited_text(Cursor::new(input), 5);
        assert_eq!(text, "hello");
        let full = read_limited_text(Cursor::new(input), 64);
        assert_eq!(full, "hello world and more");
    }

    #[test]
    fn wait_capped_child_output_captures_stdout() -> Result<(), Box<dyn std::error::Error>> {
        let mut child = Command::new("/usr/bin/printf")
            .arg("hi")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        let output = wait_capped_child_output(
            &mut child,
            CappedOutputWait {
                max_output_bytes: 64,
                timeout: Duration::from_secs(2),
                capture_stderr: false,
                drain_timeout: None,
                terminate_group_after_exit: false,
            },
            || false,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert!(output.status.success());
        assert_eq!(output.stdout, b"hi");
        Ok(())
    }

    #[test]
    fn wait_capped_child_output_times_out() -> Result<(), Box<dyn std::error::Error>> {
        let mut child = Command::new("/usr/bin/sleep")
            .arg("5")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        let result = wait_capped_child_output(
            &mut child,
            CappedOutputWait {
                max_output_bytes: 64,
                timeout: Duration::from_millis(100),
                capture_stderr: false,
                drain_timeout: None,
                terminate_group_after_exit: false,
            },
            || false,
        );
        assert!(matches!(result, Err(CappedOutputError::TimedOut)));
        Ok(())
    }
}
