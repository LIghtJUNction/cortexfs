static FIXTURE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    use std::fs;
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;

    use cortexfs::object::install::{InstallTier, install_object};
    use cortexfs::{
        AgentExecutableSocketExecution, AgentExecutableSocketRuntime, ObjectClass,
        derive_agent_runtime_view, ensure_v1_reference_tree, inspect_object_layout,
        serve_agent_executable_socket_stream_once,
    };
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn installed_sdks_run_two_declared_native_tool_calls() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = FixtureRoot::new()?;
        ensure_v1_reference_tree(root.path()).map_err(|error| {
            std::io::Error::other(format!("cannot bootstrap fixture tree: {error:?}"))
        })?;
        let package = root.path().join("package");
        fs::create_dir_all(&package)?;

        let tool = Path::new(env!("CARGO_BIN_EXE_cortexfs-sdk-fixture-tool"));
        let agent = Path::new(env!("CARGO_BIN_EXE_cortexfs-sdk-fixture-agent"));
        let tool_manifest = package.join("tool.json");
        let tool_controls = BTreeMap::from([
            ("description", "SDK fixture echo".to_owned()),
            ("schema", r#"{"type":"object"}"#.to_owned()),
            ("cap", "text".to_owned()),
            (
                "policy",
                "allow coder_t tool:example.echo execute\n".to_owned(),
            ),
        ]);
        write_manifest(&tool_manifest, "tool", "example.echo", tool, &tool_controls)
            .map_err(|_error| std::io::Error::other("installed agent runtime failed"))?;
        install_object(root.path(), &tool_manifest, InstallTier::System)?;

        let reference = root.path().join("agent/coder.d");
        let mut controls = BTreeMap::new();
        for name in [
            "owner", "uid", "gid", "groups", "label", "iso", "parent", "life", "root", "cwd",
            "env", "path", "mount", "model",
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
