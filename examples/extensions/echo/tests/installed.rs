use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::net::Shutdown;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cortexfs::object::install::{InstallTier, install_object};
use cortexfs::{
    AgentExecutableSocketRuntime, RunEnvironment, derive_agent_runtime_view, ensure_reference_tree,
    serve_agent_executable_socket_stream_once,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn canonical_echo_installs_and_runs_two_native_calls() -> Result<(), Box<dyn std::error::Error>> {
    let root = FixtureRoot::new()?;
    ensure_reference_tree(root.path()).map_err(|error| {
        io::Error::other(format!(
            "cannot bootstrap canonical echo fixture: {error:?}"
        ))
    })?;

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tool = Path::new(env!("CARGO_BIN_EXE_cortexfs-echo-tool"));
    let agent = Path::new(env!("CARGO_BIN_EXE_cortexfs-echo-agent"));
    let agent_metadata = fs::metadata(agent)?;
    let uid = agent_metadata.uid().to_string();
    let gid = agent_metadata.gid().to_string();
    let package = root.path().join("package");
    fs::create_dir_all(&package)?;

    let tool_manifest = fs::read_to_string(manifest_dir.join("tool.manifest.json.in"))?
        .replace("@TOOL_PATH@", path_text(tool)?)
        .replace("@TOOL_SHA256@", &sha256(tool)?);
    let tool_manifest_path = package.join("tool.json");
    fs::write(&tool_manifest_path, tool_manifest)?;

    let agent_manifest = fs::read_to_string(manifest_dir.join("agent.manifest.json.in"))?
        .replace("@AGENT_PATH@", path_text(agent)?)
        .replace("@AGENT_SHA256@", &sha256(agent)?)
        .replace("@UID@", &uid)
        .replace("@GID@", &gid)
        .replace("@GROUPS@", &gid);
    let mut agent_manifest: Value = serde_json::from_str(&agent_manifest)?;
    let controls = agent_manifest
        .get_mut("controls")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing agent controls"))?;
    controls.insert(
        "path".to_owned(),
        Value::String(format!("{}\n", root.path().join("tool").display())),
    );
    controls.insert(
        "mount".to_owned(),
        Value::String(format!(
            "{root}\t{root}\tro\trbind,nosuid,nodev\n",
            root = root.path().display()
        )),
    );
    let agent_manifest_path = package.join("agent.json");
    fs::write(
        &agent_manifest_path,
        serde_json::to_vec_pretty(&agent_manifest)?,
    )?;

    install_object(root.path(), &tool_manifest_path, InstallTier::System)?;
    install_object(root.path(), &agent_manifest_path, InstallTier::System)?;
    fs::write(
        root.path().join("tool/example.echo.d/policy"),
        "allow example_t tool:example.echo execute\n",
    )?;

    let view = derive_agent_runtime_view(root.path(), "example-echo").map_err(|error| {
        io::Error::other(format!(
            "cannot derive canonical echo runtime view: {error:?}"
        ))
    })?;
    let session_root = view.home().join("session");
    fs::create_dir_all(&session_root)?;
    let executable = root.path().join("agent/example-echo");
    let (mut client, mut server) = UnixStream::pair()?;
    client.write_all(
        b"{\"op\":\"send\",\"id\":\"canonical-run\",\"session\":\"default\",\"input\":\"tool hello\"}\n",
    )?;
    client.shutdown(Shutdown::Write)?;
    let outcome = serve_agent_executable_socket_stream_once(
        &mut server,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: root.path(),
            source_root: root.path(),
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/workspace",
            model: Some("main"),
            network_allowed: false,
            agent_name: "example-echo",
            agent_executable: &executable,
            environment: RunEnvironment::Native,
        },
    )
    .map_err(|error| io::Error::other(format!("canonical echo runtime failed: {error:?}")))?;

    let jsonl = outcome.jsonl();
    assert_eq!(jsonl.matches("\"type\":\"start\"").count(), 1, "{jsonl}");
    assert_eq!(jsonl.matches("\"type\":\"done\"").count(), 1, "{jsonl}");
    assert_eq!(
        jsonl.matches("\"type\":\"tool_result\"").count(),
        2,
        "{jsonl}"
    );
    assert!(jsonl.contains("echo-call-1"), "{jsonl}");
    assert!(jsonl.contains("echo-call-2"), "{jsonl}");
    assert!(!jsonl.contains("\"type\":\"error\""), "{jsonl}");

    let durable = fs::read_to_string(session_root.join("default/messages.jsonl"))?;
    let messages = durable
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    let roles = messages
        .iter()
        .map(|message| message.get("role").and_then(Value::as_str))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing durable role"))?;
    assert_eq!(roles, ["user", "tool", "tool", "assistant"]);
    assert_tool_result(
        messages.get(1).ok_or("missing first durable tool result")?,
        "echo-call-1",
        "hello",
    )?;
    assert_tool_result(
        messages
            .get(2)
            .ok_or("missing second durable tool result")?,
        "echo-call-2",
        "second:hello",
    )?;
    assert_eq!(
        messages
            .get(3)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(|part| part.get("text"))
            .and_then(Value::as_str),
        Some("second:hello")
    );
    Ok(())
}

fn assert_tool_result(
    message: &Value,
    call_id: &str,
    content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let part = message
        .get("content")
        .and_then(Value::as_array)
        .and_then(|parts| parts.first())
        .ok_or("missing durable tool result content")?;
    assert_eq!(
        part.get("tool_call_id").and_then(Value::as_str),
        Some(call_id)
    );
    assert_eq!(part.get("content").and_then(Value::as_str), Some(content));
    Ok(())
}

fn sha256(path: &Path) -> io::Result<String> {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(fs::read(path)?) {
        write!(&mut output, "{byte:02x}").map_err(io::Error::other)?;
    }
    Ok(output)
}

fn path_text(path: &Path) -> io::Result<&str> {
    path.to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 executable path"))
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new() -> io::Result<Self> {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cortexfs-canonical-echo-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.path);
    }
}
