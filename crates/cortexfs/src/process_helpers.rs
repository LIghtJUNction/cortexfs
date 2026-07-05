#[expect(
    clippy::redundant_pub_crate,
    reason = "process_helpers is shared by library modules and binaries without becoming public API"
)]
pub(crate) fn read_limited_bytes(mut reader: impl std::io::Read, limit: usize) -> Vec<u8> {
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

#[expect(
    clippy::redundant_pub_crate,
    reason = "process_helpers is shared by library modules and binaries without becoming public API"
)]
pub(crate) fn terminate_process_group(child: &mut std::process::Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        signal_process_group(pid, nix::sys::signal::Signal::SIGTERM);
        for _attempt in 0..5 {
            let _ignored = child.try_wait();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        signal_process_group(pid, nix::sys::signal::Signal::SIGKILL);
    }
    let _ignored = child.kill();
}

fn signal_process_group(pid: i32, signal: nix::sys::signal::Signal) {
    let _ignored = nix::sys::signal::kill(nix::unistd::Pid::from_raw(-pid), signal);
}
