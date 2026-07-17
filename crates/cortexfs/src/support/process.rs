use std::{io::Read, process::Child, thread, time::Duration};

/// Ordered bubblewrap process mounts appended after namespace flags.
pub const BWRAP_PROCESS_SETUP_ARGS: [&str; 8] = [
    "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--dir", "/run",
];

/// Ordered bubblewrap mounts and links for the base system layout.
pub const BWRAP_SYSTEM_LAYOUT_ARGS: [&str; 19] = [
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
    "--symlink",
    "usr/lib",
    "/lib64",
];

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
/// Prefer this when the caller only needs text; use [`read_limited_bytes`] for
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_limited_text_caps_and_decodes() {
        let input = b"hello world and more";
        let text = read_limited_text(Cursor::new(input), 5);
        assert_eq!(text, "hello");
        let full = read_limited_text(Cursor::new(input), 64);
        assert_eq!(full, "hello world and more");
    }
}
