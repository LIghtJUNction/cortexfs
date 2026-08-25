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

    fn add_provider_fixture(command: &mut Command, fixture: &Path) -> std::io::Result<()> {
        let providers = fixture.join("providers.d");
        let cache = fixture.join("provider-models");
        fs::create_dir_all(&providers)?;
        fs::create_dir_all(&cache)?;
        command
            .args(["--tmpfs", "/etc", "--dir", "/etc/profile.d"])
            .args(["--dir", "/etc/cortexfs"])
            .args(["--dir", "/etc/cortexfs/providers.d", "--bind"])
            .arg(&providers)
            .arg("/etc/cortexfs/providers.d")
            .args(["--tmpfs", "/var/lib", "--dir", "/var/lib/cortexfs"])
            .args(["--dir", "/var/lib/cortexfs/provider-models", "--bind"])
            .arg(&cache)
            .arg("/var/lib/cortexfs/provider-models");
        Ok(())
    }

    fn bootstrap_fixture(
        fixture: &Path,
        ctx: &str,
        source: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut command = Command::new("/usr/bin/bwrap");
        command.args(["--bind", "/", "/"]);
        add_provider_fixture(&mut command, fixture)?;
        let bootstrap = command
            .args(["--", ctx, "bootstrap"])
            .arg(source)
            .output()?;
        if !bootstrap.status.success() {
            return Err(String::from_utf8_lossy(&bootstrap.stderr)
                .into_owned()
                .into());
        }
        Ok(())
    }

    fn spawn_activation(
        fixture: &Path,
        source: &Path,
        socket: &Path,
        runtime: &str,
        agent: &str,
    ) -> std::io::Result<ChildGuard> {
        let mut command = Command::new("/usr/bin/bwrap");
        command.args([
            "--bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--dev-bind",
            "/dev",
            "/dev",
            "--tmpfs",
            "/run",
            "--dir",
            "/run/cortexfs",
            "--dir",
            "/run/cortexfs/control",
        ]);
        add_provider_fixture(&mut command, fixture)?;
        command
            .args([
                "--",
                "/usr/bin/systemd-socket-activate",
                "-l",
                socket
                    .to_str()
                    .ok_or_else(|| std::io::Error::other("invalid socket"))?,
                runtime,
                "--source",
            ])
            .arg(source)
            .args(["--agent", agent])
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map(|child| ChildGuard(Some(child)))
    }

    fn wait_for_socket(
        activation: &mut ChildGuard,
        socket: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
        Ok(())
    }

    #[test]
    fn socket_activated_runtime_ping_does_not_execute_agent_or_provider()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let socket = root.path().join("executor.sock");
        let agent_marker = root.path().join("agent-called");
        let provider_marker = root.path().join("provider-called");
        let ctx = env!("CARGO_BIN_EXE_ctx");
        let runtime = std::env::var("CARGO_BIN_EXE_cortexfs-agent-runtime")?;
        for program in [
            "/usr/bin/bwrap",
            "/usr/bin/systemd-socket-activate",
            runtime.as_str(),
        ] {
            require_program(Path::new(program))?;
        }

        bootstrap_fixture(root.path(), ctx, &source)?;
        let agent = source.join("agent/executor");
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
        fs::write(source.join("agent/executor.d/model"), "debug/echo\n")?;
        fs::write(
            source.join("agent/executor.d/policy"),
            "allow executor_t model:debug/echo use\nallow executor_t tool:tsh execute\n",
        )?;

        let mut activation =
            spawn_activation(root.path(), &source, &socket, runtime.as_str(), "executor")?;
        wait_for_socket(&mut activation, &socket)?;
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

    #[test]
    fn socket_activated_generated_wrapper_is_terminal_and_restartable()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let ctx = env!("CARGO_BIN_EXE_ctx");
        let runtime = std::env::var("CARGO_BIN_EXE_cortexfs-agent-runtime")?;
        for program in [
            "/usr/bin/bwrap",
            "/usr/bin/systemd-socket-activate",
            runtime.as_str(),
        ] {
            require_program(Path::new(program))?;
        }
        bootstrap_fixture(root.path(), ctx, &source)?;
        // Wrappers exec /ctx/bin/cortexfs-object-runner; plant the workspace-built
        // runner so envelope ABI matches this tree (system /usr/bin may lag).
        let runner = std::env::var("CARGO_BIN_EXE_cortexfs-object-runner")?;
        fs::copy(&runner, source.join("bin/cortexfs-object-runner"))?;
        fs::set_permissions(
            source.join("bin/cortexfs-object-runner"),
            fs::Permissions::from_mode(0o755),
        )?;
        let agent = source.join("agent/executor");
        assert!(fs::read_to_string(&agent)?.contains("cortexfs-object-runner"));
        fs::write(source.join("agent/executor.d/model"), "debug/echo\n")?;
        let policy = source.join("agent/executor.d/policy");
        fs::write(
            &policy,
            format!(
                "{}allow executor_t model:debug/echo use\n",
                fs::read_to_string(&policy)?
            ),
        )?;

        for attempt in 1..=7 {
            let socket = root.path().join(format!("executor-{attempt}.sock"));
            let mut activation =
                spawn_activation(root.path(), &source, &socket, runtime.as_str(), "executor")?;
            wait_for_socket(&mut activation, &socket)?;
            let mut stream = UnixStream::connect(&socket)?;
            writeln!(
                stream,
                "{{\"op\":\"send\",\"id\":\"wrapper-{attempt}\",\"session\":\"default\",\"input\":\"hello\"}}"
            )?;
            stream.shutdown(std::net::Shutdown::Write)?;
            let mut response = String::new();
            BufReader::new(stream).read_to_string(&mut response)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            let status = loop {
                if let Some(status) = activation.child()?.try_wait()? {
                    break status;
                }
                if Instant::now() >= deadline {
                    let stderr = activation.terminate_with_stderr();
                    return Err(format!("activation did not exit: {stderr}").into());
                }
                thread::sleep(Duration::from_millis(10));
            };
            let stderr = activation.terminate_with_stderr();
            assert!(status.success(), "stderr={stderr}; response={response}");
            let done = response
                .lines()
                .filter(|line| {
                    serde_json::from_str::<serde_json::Value>(line).is_ok_and(|value| {
                        value.get("type").and_then(serde_json::Value::as_str) == Some("done")
                    })
                })
                .collect::<Vec<_>>();
            assert_eq!(done.len(), 1, "stderr={stderr}; response={response}");
            assert!(
                done.first()
                    .is_some_and(|line| line.contains("\"status\":\"ok\"")),
                "stderr={stderr}; response={response}"
            );
            let state = source
                .join("home")
                .join(nix::unistd::geteuid().as_raw().to_string())
                .join("agent/executor/session/default/state");
            assert_eq!(fs::read_to_string(state)?, "done\n");
        }
        Ok(())
    }
}
