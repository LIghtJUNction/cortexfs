static FIXTURE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    use std::fs;
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::Ordering;

    use cortexfs::object::install::{InstallTier, install_object};
    use cortexfs::{
        AgentExecutableSocketExecution, AgentExecutableSocketRuntime, ObjectClass,
        derive_agent_runtime_view, ensure_v1_reference_tree, ensure_v1_runtime_models,
        inspect_object_layout, serve_agent_executable_socket_stream_once,
    };
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn installed_sdks_run_two_declared_native_tool_calls() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = install_fixture_agent()?;

        let view = derive_agent_runtime_view(root.path(), "fixture-agent").map_err(|error| {
            std::io::Error::other(format!("cannot derive fixture runtime view: {error:?}"))
        })?;
        let session_root = view.home().join("session");
        fs::create_dir_all(&session_root)?;
        let (mut client, mut server) = UnixStream::pair()?;
        client
        .write_all(
            b"{\"op\":\"send\",\"id\":\"fixture-run\",\"session\":\"default\",\"input\":\"go\"}\n",
        )
        .map_err(|_error| std::io::Error::other("installed agent runtime failed"))?;
        client.shutdown(std::net::Shutdown::Write)?;
        let executable = root.path().join("agent/fixture-agent");
        let outcome = serve_agent_executable_socket_stream_once(
            &mut server,
            None,
            AgentExecutableSocketRuntime {
                ctx_root: root.path(),
                source_root: root.path(),
                identity: view.identity(),
                env: view.env(),
                session_root: &session_root,
                default_cwd: "/work",
                model: Some("debug/echo"),
                network_allowed: false,
                agent_name: "fixture-agent",
                agent_executable: &executable,
                execution: AgentExecutableSocketExecution::Direct,
            },
        )
        .map_err(|_error| std::io::Error::other("cannot execute fixture socket run"))?;
        assert_eq!(
            outcome.jsonl().matches("\"type\":\"tool_result\"").count(),
            2,
            "{}",
            outcome.jsonl()
        );

        let durable = fs::read_to_string(session_root.join("default/messages.jsonl"))?;
        let mut tool_results = durable
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|frame| frame.get("role").and_then(Value::as_str) == Some("tool"));
        let first = tool_results.next().ok_or("missing first tool result")?;
        let second = tool_results.next().ok_or("missing second tool result")?;
        assert!(tool_results.next().is_none());
        assert_tool_result(&first, "fixture-call-1", "native:one")?;
        assert_tool_result(&second, "fixture-call-2", "native:two")?;
        Ok(())
    }

    #[test]
    fn installed_agent_cli_joins_argv_and_emits_success_jsonl()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = install_fixture_agent()?;
        let output = run_installed_agent(&root, &["hello", "world"], b"", "agent-cli-argv")?;

        assert_eq!(
            (
                output.status.code(),
                String::from_utf8(output.stdout)?,
                output.stderr,
            ),
            (
                Some(0),
                concat!(
                    "{\"run\":\"agent-cli-argv\",\"type\":\"start\"}\n",
                    "{\"content\":[{\"text\":\"hello world\",\"type\":\"text\"}],\"role\":\"assistant\",\"run\":\"agent-cli-argv\",\"type\":\"message\"}\n",
                    "{\"run\":\"agent-cli-argv\",\"status\":\"ok\",\"type\":\"done\"}\n",
                )
                .to_owned(),
                Vec::new(),
            )
        );
        Ok(())
    }

    #[test]
    fn installed_agent_cli_reads_stdin_when_argv_is_empty() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = install_fixture_agent()?;
        let output = run_installed_agent(&root, &[], b"from stdin", "agent-cli-stdin")?;

        assert_eq!(
            (
                output.status.code(),
                String::from_utf8(output.stdout)?,
                output.stderr,
            ),
            (
                Some(0),
                concat!(
                    "{\"run\":\"agent-cli-stdin\",\"type\":\"start\"}\n",
                    "{\"content\":[{\"text\":\"from stdin\",\"type\":\"text\"}],\"role\":\"assistant\",\"run\":\"agent-cli-stdin\",\"type\":\"message\"}\n",
                    "{\"run\":\"agent-cli-stdin\",\"status\":\"ok\",\"type\":\"done\"}\n",
                )
                .to_owned(),
                Vec::new(),
            )
        );
        Ok(())
    }

    #[test]
    fn installed_tool_cli_joins_argv_and_emits_success_jsonl()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = install_fixture_tool()?;
        let output = run_installed_tool(&root, &["hello", "world"], b"", "cli-argv")?;

        assert_eq!(
            (
                output.status.code(),
                String::from_utf8(output.stdout)?,
                output.stderr,
            ),
            (
                Some(0),
                concat!(
                    "{\"run\":\"cli-argv\",\"tool\":\"example.echo\",\"type\":\"start\"}\n",
                    "{\"content\":[{\"text\":\"native:hello world\",\"type\":\"text\"}],\"role\":\"tool\",\"run\":\"cli-argv\",\"type\":\"message\"}\n",
                    "{\"run\":\"cli-argv\",\"status\":\"ok\",\"type\":\"done\"}\n",
                )
                .to_owned(),
                Vec::new(),
            )
        );
        Ok(())
    }

    #[test]
    fn installed_tool_cli_reads_stdin_when_argv_is_empty() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = install_fixture_tool()?;
        let output = run_installed_tool(&root, &[], b"from stdin", "cli-stdin")?;

        assert_eq!(
            (
                output.status.code(),
                String::from_utf8(output.stdout)?,
                output.stderr,
            ),
            (
                Some(0),
                concat!(
                    "{\"run\":\"cli-stdin\",\"tool\":\"example.echo\",\"type\":\"start\"}\n",
                    "{\"content\":[{\"text\":\"native:from stdin\",\"type\":\"text\"}],\"role\":\"tool\",\"run\":\"cli-stdin\",\"type\":\"message\"}\n",
                    "{\"run\":\"cli-stdin\",\"status\":\"ok\",\"type\":\"done\"}\n",
                )
                .to_owned(),
                Vec::new(),
            )
        );
        Ok(())
    }

    #[test]
    fn installed_tool_cli_error_emits_canonical_jsonl_and_exits_one()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = install_fixture_tool()?;
        let output = run_installed_tool(&root, &["__error__"], b"", "cli-error")?;

        assert_eq!(
            (
                output.status.code(),
                String::from_utf8(output.stdout)?,
                output.stderr,
            ),
            (
                Some(1),
                concat!(
                    "{\"run\":\"cli-error\",\"tool\":\"example.echo\",\"type\":\"start\"}\n",
                    "{\"code\":\"EINVAL\",\"message\":\"fixture failure\",\"run\":\"cli-error\",\"type\":\"error\"}\n",
                    "{\"run\":\"cli-error\",\"status\":\"error\",\"type\":\"done\"}\n",
                )
                .to_owned(),
                Vec::new(),
            )
        );
        Ok(())
    }

    #[test]
    fn installed_tool_cli_rejects_oversized_stdin_before_jsonl()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = install_fixture_tool()?;
        let stdin = vec![b'x'; 1024 * 1024 + 1];
        let output = run_installed_tool(&root, &[], &stdin, "cli-oversized")?;

        assert_eq!(
            (output.status.code(), output.stdout, output.stderr),
            (Some(1), Vec::new(), Vec::new())
        );
        Ok(())
    }

    fn assert_tool_result(
        frame: &Value,
        call_id: &str,
        content: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let item = frame
            .get("content")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .ok_or("missing durable tool result")?;
        assert_eq!(
            item.get("tool_call_id").and_then(Value::as_str),
            Some(call_id)
        );
        assert_eq!(item.get("content").and_then(Value::as_str), Some(content));
        Ok(())
    }

    fn install_fixture_tool() -> Result<FixtureRoot, Box<dyn std::error::Error>> {
        let root = FixtureRoot::new()?;
        ensure_v1_reference_tree(root.path()).map_err(|error| {
            std::io::Error::other(format!("cannot bootstrap fixture tree: {error:?}"))
        })?;
        let package = root.path().join("package");
        fs::create_dir_all(&package)?;
        let manifest = package.join("tool.json");
        let controls = BTreeMap::from([
            ("description", "SDK fixture echo".to_owned()),
            ("schema", r#"{"type":"object"}"#.to_owned()),
            ("cap", "text".to_owned()),
            (
                "policy",
                "allow coder_t tool:example.echo execute\n".to_owned(),
            ),
        ]);
        write_manifest(
            &manifest,
            "tool",
            "example.echo",
            Path::new(env!("CARGO_BIN_EXE_cortexfs-sdk-fixture-tool")),
            &controls,
        )?;
        install_object(root.path(), &manifest, InstallTier::System)?;
        Ok(root)
    }

    fn install_fixture_agent() -> Result<FixtureRoot, Box<dyn std::error::Error>> {
        let root = install_fixture_tool()?;
        ensure_v1_runtime_models(root.path()).map_err(|error| {
            std::io::Error::other(format!(
                "cannot materialize fixture runtime models: {error:?}"
            ))
        })?;
        let package = root.path().join("package");
        let agent = Path::new(env!("CARGO_BIN_EXE_cortexfs-sdk-fixture-agent"));
        let reference = root.path().join("agent/coder.d");
        let mut controls = BTreeMap::new();
        for name in [
            "owner", "uid", "gid", "groups", "label", "iso", "parent", "life", "root", "cwd",
            "env", "path", "mount", "model", "window",
        ] {
            controls.insert(name, fs::read_to_string(reference.join(name))?);
        }
        controls.insert("path", format!("{}\n", root.path().join("tool").display()));
        controls.insert(
            "mount",
            format!(
                "{}\t{}\tro\trbind,nosuid,nodev\n",
                root.path().display(),
                root.path().display()
            ),
        );
        controls.insert("abi", "sdk-envelope-v1".to_owned());
        controls.insert("tools", "example.echo\n".to_owned());
        controls.insert(
            "policy",
            "allow coder_t model:main use\nallow coder_t tool:example.echo execute\n".to_owned(),
        );
        let agent_manifest = package.join("agent.json");
        write_manifest(&agent_manifest, "agent", "fixture-agent", agent, &controls)?;
        install_object(root.path(), &agent_manifest, InstallTier::System)?;
        assert!(inspect_object_layout(root.path(), ObjectClass::Agent, "fixture-agent").is_ok());
        Ok(root)
    }

    fn run_installed_tool(
        root: &FixtureRoot,
        args: &[&str],
        stdin: &[u8],
        run_id: &str,
    ) -> Result<Output, Box<dyn std::error::Error>> {
        let mut child = Command::new(root.path().join("tool/example.echo"))
            .args(args)
            .env_clear()
            .env("CTX_RUN_ID", run_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("missing fixture tool stdin")?
            .write_all(stdin)?;
        Ok(child.wait_with_output()?)
    }

    fn run_installed_agent(
        root: &FixtureRoot,
        args: &[&str],
        stdin: &[u8],
        run_id: &str,
    ) -> Result<Output, Box<dyn std::error::Error>> {
        let mut child = Command::new(root.path().join("agent/fixture-agent"))
            .args(args)
            .env_clear()
            .env("CTX_AGENT", "fixture-agent")
            .env("CTX_RUN_ID", run_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("missing fixture agent stdin")?
            .write_all(stdin)?;
        Ok(child.wait_with_output()?)
    }

    fn write_manifest(
        path: &Path,
        class: &str,
        name: &str,
        executable: &Path,
        controls: &BTreeMap<&str, String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let digest = Sha256::digest(fs::read(executable)?).iter().fold(
            String::with_capacity(64),
            |mut output, byte| {
                let _ignored = write!(output, "{byte:02x}");
                output
            },
        );
        fs::write(
            path,
            json!({
                "schema": "cortexfs.object/v1",
                "class": class,
                "name": name,
                "executable": { "path": executable, "sha256": digest },
                "controls": controls,
            })
            .to_string(),
        )?;
        Ok(())
    }

    struct FixtureRoot(PathBuf);

    impl FixtureRoot {
        fn new() -> Result<Self, std::io::Error> {
            let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "cortexfs-sdk-extension-fixture-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self(path))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for FixtureRoot {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.0);
        }
    }
}
