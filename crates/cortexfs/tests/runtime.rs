#![cfg(target_os = "linux")]

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    struct ChildGuard(Option<Child>);

    impl ChildGuard {
        fn child(&mut self) -> std::io::Result<&mut Child> {
            self.0
                .as_mut()
                .ok_or_else(|| std::io::Error::other("child already reaped"))
        }

        fn terminate(&mut self) {
            let Some(child) = self.0.as_mut() else {
                return;
            };
            if let Ok(pid) = i32::try_from(child.id()) {
                let group = nix::unistd::Pid::from_raw(-pid);
                let _ignored = nix::sys::signal::kill(group, nix::sys::signal::Signal::SIGTERM);
                for _attempt in 0..5 {
                    if child.try_wait().is_ok_and(|status| status.is_some()) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                let _ignored = nix::sys::signal::kill(group, nix::sys::signal::Signal::SIGKILL);
            }
            let _ignored = child.kill();
            let _ignored = child.wait();
        }

        fn terminate_with_stderr(&mut self) -> String {
            const STDERR_LIMIT: u64 = 8 * 1024;

            self.terminate();
            let Some(stderr) = self.0.as_mut().and_then(|child| child.stderr.take()) else {
                return String::new();
            };
            let mut output = String::new();
            let _ignored = stderr.take(STDERR_LIMIT).read_to_string(&mut output);
            output
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            self.terminate();
        }
    }

    fn require_program(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = fs::metadata(path)
            .map_err(|error| format!("required test program {}: {error}", path.display()))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!(
                "required test program is not executable: {}",
                path.display()
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn socket_activated_runtime_ping_does_not_execute_agent_or_provider()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let socket = root.path().join("coder.sock");
        let agent_marker = root.path().join("agent-called");
        let provider_marker = root.path().join("provider-called");
        let ctx = env!("CARGO_BIN_EXE_ctx");
        let runtime = env!("CARGO_BIN_EXE_cortexfs-agent-runtime");
        for program in [
            "/usr/bin/unshare",
            "/bin/sh",
            "/usr/bin/mount",
            "/usr/bin/install",
            "/usr/bin/systemd-socket-activate",
            runtime,
        ] {
            require_program(Path::new(program))?;
        }

        let bootstrap = Command::new(ctx)
            .args(["bootstrap", source.to_str().ok_or("source")?])
            .output()?;
        if !bootstrap.status.success() {
            return Err(String::from_utf8_lossy(&bootstrap.stderr)
                .into_owned()
                .into());
        }
        let agent = source.join("agent/coder");
        fs::write(
            &agent,
            format!("#!/bin/sh\ntouch {}\n", agent_marker.display()),
        )?;
        fs::set_permissions(&agent, fs::Permissions::from_mode(0o755))?;
        let provider = source.join("model/debug/echo");
        if let Some(parent) = provider.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &provider,
            format!("#!/bin/sh\ntouch {}\n", provider_marker.display()),
        )?;
        fs::set_permissions(&provider, fs::Permissions::from_mode(0o755))?;
        fs::write(source.join("agent/coder.d/model"), "debug/echo\n")?;
        fs::write(
            source.join("agent/coder.d/policy"),
            "allow coder_t model:debug/echo use\nallow coder_t tool:tsh execute\n",
        )?;

        let mut activation = ChildGuard(Some(
        Command::new("/usr/bin/unshare")
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--fork",
            "/bin/sh",
            "-c",
                "/usr/bin/mount -t tmpfs tmpfs /run/cortexfs && /usr/bin/install -d -m 0711 /run/cortexfs/control && exec /usr/bin/systemd-socket-activate \"$@\"",
            "cortexfs-runtime-test",
            "-l",
            socket.to_str().ok_or("socket")?,
            runtime,
            "--source",
        ])
            .arg(&source)
            .args(["--agent", "coder"])
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?,
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !socket.exists() {
            if let Some(status) = activation.child()?.try_wait()? {
                let stderr = activation.terminate_with_stderr();
                return Err(format!(
                    "socket activation prerequisites unavailable or child exited {status}: {stderr}"
                )
                .into());
            }
            if Instant::now() >= deadline {
                let stderr = activation.terminate_with_stderr();
                return Err(format!("socket activation timed out: {stderr}").into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        let mut stream = UnixStream::connect(&socket).map_err(|error| {
            let stderr = activation.terminate_with_stderr();
            format!("cannot connect activated socket: {error}; activation stderr: {stderr}")
        })?;
        stream.write_all(b"{\"op\":\"ping\"}\n")?;
        stream.shutdown(std::net::Shutdown::Write)?;
        let mut response = String::new();
        if let Err(error) = BufReader::new(stream).read_line(&mut response) {
            let stderr = activation.terminate_with_stderr();
            return Err(format!("ping read failed: {error}; activation stderr: {stderr}").into());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = activation.child()?.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                activation.child()?.kill()?;
                return Err("socket-activated runtime did not exit".into());
            }
            thread::sleep(Duration::from_millis(10));
        };

        assert!(status.success());
        assert!(response.contains("\"type\":\"pong\""));
        assert!(!agent_marker.exists());
        assert!(!provider_marker.exists());
        Ok(())
    }
}
