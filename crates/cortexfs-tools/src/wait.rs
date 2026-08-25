use crate::waitread::{join_reader, read_capped, sleep_tick, take_finished};
use nix::sys::signal::Signal;
use std::io;
use std::process::{Child, Output};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) enum WaitError {
    Wait(io::Error),
    ExceededLimit,
    TimedOut,
}

pub(crate) fn wait_capped_child_output(
    child: &mut Child,
    max_output_bytes: usize,
    timeout: Duration,
) -> Result<Output, WaitError> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| WaitError::Wait(io::Error::other("missing stdout pipe")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| WaitError::Wait(io::Error::other("missing stderr pipe")))?;
    let mut stdout_reader = Some(thread::spawn(move || read_capped(stdout, max_output_bytes)));
    let mut stderr_reader = Some(thread::spawn(move || read_capped(stderr, max_output_bytes)));
    let mut stdout_buffer = None;
    let mut stderr_buffer = None;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if take_finished(&mut stdout_reader, &mut stdout_buffer, max_output_bytes)
            || take_finished(&mut stderr_reader, &mut stderr_buffer, max_output_bytes)
        {
            abort_child(child, &mut stdout_reader, &mut stderr_reader);
            return Err(WaitError::ExceededLimit);
        }
        if let Some(status) = child.try_wait().map_err(WaitError::Wait)? {
            break status;
        }
        if Instant::now() >= deadline {
            abort_child(child, &mut stdout_reader, &mut stderr_reader);
            return Err(WaitError::TimedOut);
        }
        sleep_tick();
    };
    let stdout = join_reader(stdout_reader, stdout_buffer);
    let stderr = join_reader(stderr_reader, stderr_buffer);
    if stdout.len() > max_output_bytes || stderr.len() > max_output_bytes {
        return Err(WaitError::ExceededLimit);
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn abort_child(
    child: &mut Child,
    stdout_reader: &mut Option<thread::JoinHandle<Vec<u8>>>,
    stderr_reader: &mut Option<thread::JoinHandle<Vec<u8>>>,
) {
    terminate_process_group(child);
    let _ignored = child.wait();
    drop(stdout_reader.take());
    drop(stderr_reader.take());
}

fn terminate_process_group(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        let pid = nix::unistd::Pid::from_raw(-pid);
        let _ignored = nix::sys::signal::kill(pid, Signal::SIGTERM);
        for _attempt in 0..5 {
            let _ignored = child.try_wait();
            thread::sleep(Duration::from_millis(50));
        }
        let _ignored = nix::sys::signal::kill(pid, Signal::SIGKILL);
    }
    let _ignored = child.kill();
}
