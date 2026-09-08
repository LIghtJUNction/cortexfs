use super::{
    CappedOutputError, CappedOutputWait, read_limited_text, terminate_process_group,
    wait_capped_child_output, write_child_input,
};
use std::io::Cursor;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::{io, sync::mpsc, time::Duration};

fn output_wait(timeout: Duration) -> CappedOutputWait {
    CappedOutputWait {
        max_output_bytes: 64,
        timeout,
        capture_stderr: false,
        drain_timeout: None,
        terminate_group_after_exit: false,
    }
}

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
    let output =
        wait_capped_child_output(&mut child, output_wait(Duration::from_secs(2)), || false)
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
    let result =
        wait_capped_child_output(&mut child, output_wait(Duration::from_millis(100)), || {
            false
        });
    assert!(matches!(result, Err(CappedOutputError::TimedOut)));
    Ok(())
}

#[test]
fn blocked_child_input_is_released_on_cancellation() -> Result<(), Box<dyn std::error::Error>> {
    let mut child = Command::new("/usr/bin/sleep")
        .arg("30")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let input = write_child_input(&mut child, vec![b'x'; 512 * 1024])?;
    let result = wait_capped_child_output(&mut child, output_wait(Duration::from_secs(5)), || true);
    assert!(matches!(result, Err(CappedOutputError::Cancelled)));
    assert_eq!(
        input
            .result
            .recv_timeout(Duration::from_secs(1))?
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::BrokenPipe)
    );
    assert!(child.try_wait()?.is_some());
    Ok(())
}

#[test]
fn dropping_child_input_stops_writer_with_reader_still_alive()
-> Result<(), Box<dyn std::error::Error>> {
    let mut child = Command::new("/usr/bin/sleep")
        .arg("30")
        .stdin(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let mut input = write_child_input(&mut child, vec![b'x'; 512 * 1024])?;
    let pending = input.result.recv_timeout(Duration::from_millis(50));
    let result = std::mem::replace(&mut input.result, mpsc::channel().1);
    drop(input);
    let stopped = result.recv_timeout(Duration::from_secs(1));
    let reader_alive = child.try_wait()?;
    terminate_process_group(&mut child);
    let _status = child.wait()?;
    assert!(matches!(pending, Err(mpsc::RecvTimeoutError::Timeout)));
    assert_eq!(
        stopped?.err().map(|error| error.kind()),
        Some(io::ErrorKind::Interrupted)
    );
    assert!(reader_alive.is_none());
    Ok(())
}
