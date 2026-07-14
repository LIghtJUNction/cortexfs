use std::io::{self, Read, Write};
use std::process::{Command, Stdio};

const MAX_CLIPBOARD_BYTES: usize = 256 * 1024;

pub(crate) fn read() -> io::Result<String> {
    for (program, args) in readers() {
        let Ok(mut child) = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .spawn()
        else {
            continue;
        };
        let mut stdout = Vec::new();
        let read = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("clipboard stdout unavailable"))?
            .take(
                u64::try_from(MAX_CLIPBOARD_BYTES)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            )
            .read_to_end(&mut stdout);
        if read.is_err() || stdout.len() > MAX_CLIPBOARD_BYTES {
            let _ignored = child.kill();
            let _ignored = child.wait();
            continue;
        }
        if child.wait().is_ok_and(|status| status.success()) {
            return String::from_utf8(stdout).map_err(|_error| {
                io::Error::new(io::ErrorKind::InvalidData, "clipboard is not UTF-8")
            });
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no working clipboard backend (wl-paste, xclip, xsel)",
    ))
}

pub(crate) fn write(text: &str) -> io::Result<()> {
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "clipboard text is too large",
        ));
    }
    for (program, args) in writers() {
        let Ok(mut child) = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()
        else {
            continue;
        };
        if child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok())
            && child.wait().is_ok_and(|status| status.success())
        {
            return Ok(());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no working clipboard backend (wl-copy, xclip, xsel)",
    ))
}

pub(crate) fn readers() -> [(&'static str, &'static [&'static str]); 3] {
    [
        ("wl-paste", &["--no-newline"]),
        ("xclip", &["-selection", "clipboard", "-o"]),
        ("xsel", &["--clipboard", "--output"]),
    ]
}

pub(crate) fn writers() -> [(&'static str, &'static [&'static str]); 3] {
    [
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard", "-i"]),
        ("xsel", &["--clipboard", "--input"]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_backends_have_fixed_safe_argv() {
        assert_eq!(readers().first().map(|entry| entry.0), Some("wl-paste"));
        assert_eq!(writers().first().map(|entry| entry.0), Some("wl-copy"));
        assert!(
            readers()
                .into_iter()
                .chain(writers())
                .flat_map(|entry| entry.1)
                .all(|arg| !arg.contains(';') && !arg.contains("$("))
        );
    }
}
