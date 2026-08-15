#![cfg(test)]

use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const SECRET: &str = "ctxmcp-secret-must-not-leak";
const UNDECLARED_SECRET: &str = "must-not-reach-mcp";

fn config(path: &Path, observed: &Path) -> io::Result<()> {
    let script = r#"
import json, os, sys
secret=os.environ.get("MCP_SECRET","")
with open(os.environ["OBSERVED_ENV"],"w") as f:
 json.dump({"undeclared":os.environ.get("UNDECLARED_SECRET"),"declared":secret,"path":os.environ.get("PATH")},f)
for line in sys.stdin:
 r=json.loads(line)
 if "id" not in r: continue
 m=r.get("method")
 if m=="initialize":
  assert r.get("params",{}).get("capabilities")=={}
  result={"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"mock","version":"1"}}
 elif m=="tools/list": result={"tools":[{"name":"echo","title":"Echo title","description":"Echo safely","icons":[{"src":"data:image/svg+xml,echo","mimeType":"image/svg+xml","sizes":["any"]}],"annotations":{"readOnlyHint":True},"inputSchema":{"type":"object","properties":{"text":{"type":"string"}}},"outputSchema":{"type":"object"},"execution":{"taskSupport":"forbidden"},"_meta":{"fixture":"e2e"}}]}
 elif m=="tools/call": result={"content":[{"type":"text","text":"remote-ok"}],"isError":False}
 else: result={}
 print(json.dumps({"jsonrpc":"2.0","id":r["id"],"result":result}),flush=True)
"#;
    let value = json!({"mcpServers":{"demo":{"command":"/usr/bin/python3","args":["-u","-c",script],"env":{"MCP_SECRET":SECRET,"OBSERVED_ENV":observed}}}});
    fs::write(path, serde_json::to_vec(&value)?)
}

fn assert_no_secret(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    assert!(!text.contains(SECRET));
    assert!(!text.contains(UNDECLARED_SECRET));
}

#[derive(Clone, Copy)]
struct TestIdentity {
    uid: u32,
    gid: u32,
}

fn test_identity(path: &Path) -> io::Result<TestIdentity> {
    let metadata = fs::metadata(path)?;
    Ok(TestIdentity {
        uid: metadata.uid(),
        gid: metadata.gid(),
    })
}

fn agent_fixture(
    root: &Path,
    identity: TestIdentity,
    agent_allows: bool,
    tool_allows: bool,
) -> io::Result<()> {
    let control = root.join("agent/coder.d");
    fs::create_dir_all(&control)?;
    let execute = "allow coder_t tool:demo.echo execute\n";
    for (name, value) in [
        ("owner", identity.uid),
        ("uid", identity.uid),
        ("gid", identity.gid),
        ("groups", identity.gid),
    ] {
        fs::write(control.join(name), format!("{value}\n"))?;
    }
    for (name, value) in [
        ("abi", "sdk-envelope-v1\n"),
        ("label", "user_u:agent_r:coder_t:s0\n"),
        ("iso", "shared\n"),
        ("parent", "\n"),
        ("life", "owned\n"),
        ("root", "/\n"),
        ("cwd", "/workspace\n"),
        ("env", "\n"),
        ("model", "main\n"),
        ("window", "auto\n"),
    ] {
        fs::write(control.join(name), value)?;
    }
    fs::write(
        control.join("path"),
        format!(
            "{}:{}\n",
            root.join("tool").display(),
            root.join("home")
                .join(identity.uid.to_string())
                .join("tool")
                .display()
        ),
    )?;
    fs::write(
        control.join("mount"),
        format!(
            "{}\t{}\tro\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    )?;
    fs::write(
        control.join("policy"),
        format!(
            "allow coder_t model:main use\n{}",
            if agent_allows { execute } else { "" }
        ),
    )?;
    fs::write(
        root.join("tool/demo.echo.d/policy"),
        if tool_allows { execute } else { "" },
    )?;
    let model = root.join("model/local/chat.d");
    fs::create_dir_all(&model)?;
    fs::write(model.join("limit"), "unknown\n")?;
    let main = root.join("model/main");
    if fs::symlink_metadata(&main).is_err() {
        std::os::unix::fs::symlink("/ctx/model/local/chat", main)?;
    }
    let session = root
        .join("home")
        .join(identity.uid.to_string())
        .join("agent/coder/session/live");
    fs::create_dir_all(&session)?;
    fs::write(session.join("current_run"), "mcp-e2e\n")
}

fn project_and_install(root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let config_path = root.join("mcp.json");
    let observed = root.join("observed.json");
    let out = root.join("projected");
    let source = root.join("source");
    fs::create_dir_all(source.join("tool"))?;
    config(&config_path, &observed)?;
    let binary = Path::new(env!("CARGO_BIN_EXE_ctxmcp"));

    let listed = Command::new(binary)
        .args(["list", "--config"])
        .arg(&config_path)
        .args(["--server", "demo"])
        .env("UNDECLARED_SECRET", UNDECLARED_SECRET)
        .output()?;
    assert!(listed.status.success(), "{listed:?}");
    assert_no_secret(&listed.stdout);
    assert_no_secret(&listed.stderr);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("demo.echo\tEcho safely"));
    let environment: Value = serde_json::from_slice(&fs::read(&observed)?)?;
    assert_eq!(environment.get("undeclared"), Some(&Value::Null));
    assert_eq!(
        environment.get("declared").and_then(Value::as_str),
        Some(SECRET)
    );
    assert_eq!(
        environment.get("path").and_then(Value::as_str),
        Some("/usr/bin:/bin")
    );

    let projected = Command::new(binary)
        .args(["project", "--config"])
        .arg(&config_path)
        .arg("--runtime-config")
        .arg(&config_path)
        .args(["--server", "demo", "--out"])
        .arg(&out)
        .output()?;
    assert!(projected.status.success(), "{projected:?}");
    assert_no_secret(&projected.stdout);
    assert_no_secret(&projected.stderr);
    let manifest = out.join("demo.echo.manifest.json");
    let manifest_bytes = fs::read(&manifest)?;
    assert_no_secret(&manifest_bytes);
    assert!(cortexfs::object::install::check_object(&manifest).is_ok());
    cortexfs::object::install::install_object(
        &source,
        &manifest,
        cortexfs::object::install::InstallTier::System,
    )
    .map_err(|error| io::Error::other(error.message()))?;
    let tool_path = cortexfs::ToolPath::new([source.join("tool")]);
    let hit = tool_path
        .find("demo.echo")
        .map_err(|error| io::Error::other(format!("{error:?}")))?
        .ok_or("installed tool not found")?;
    assert!(hit.control_dir().join("mcp").is_file());

    Ok(source)
}

fn call_tool(root: &Path, name: &str) -> Result<(std::process::Output, PathBuf), String> {
    let view = cortexfs::derive_agent_runtime_view(root, "coder")
        .map_err(|error| format!("agent fixture does not form a runtime view: {error:?}"))?;
    let hit = view
        .tool_path()
        .find(name)
        .map_err(|error| format!("fixture CTX_PATH is not readable: {error:?}"))?
        .ok_or_else(|| "installed tool is not visible".to_owned())?;
    let policy = cortexfs::support::plain::read_small_text_file(
        &hit.control_dir().join("policy"),
        64 * 1024,
    )
    .map_err(|error| format!("installed tool policy is not readable: {error}"))?;
    let tool_policy = cortexfs::PolicyV0::parse(&policy)
        .map_err(|error| format!("tool policy is invalid: {error:?}"))?;
    let grant = cortexfs::authorize_tool_execution(
        view.tool_path(),
        name,
        cortexfs::ToolExecutionAuthority::new(
            view.identity(),
            view.mount_table(),
            view.policy_subject(),
            view.policy(),
            &tool_policy,
        ),
    )
    .map_err(|denial| format!("{}: {denial:?}", denial.errno()))?;
    if grant.hit() != &hit {
        return Err("authorization changed the CTX_PATH hit".to_owned());
    }
    let executable = cortexfs::cli::nofollow::open_executable_no_follow(grant.hit().path())
        .map_err(|error| format!("authorized tool is not a plain executable: {error}"))?;
    let output = Command::new(cortexfs::cli::procfd::proc_fd_path(&executable))
        .arg(r#"{"text":"hello"}"#)
        .env_clear()
        .envs(view.env().iter().cloned())
        .env("CTX_SOURCE", root)
        .env("CTX_AGENT", "coder")
        .env("CTX_SESSION", "live")
        .env("CTX_RUN_ID", "mcp-e2e")
        .env("CTX_TOOL_MODE", "cli")
        .env("CTX_AUTHORIZED_OBJECT", grant.hit().path())
        .env("UNDECLARED_SECRET", UNDECLARED_SECRET)
        .output()
        .map_err(|error| format!("authorized ctxmcp did not start: {error}"))?;
    Ok((output, grant.hit().path().to_owned()))
}

fn exercise_remote_tool(root: &Path, source: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let identity = test_identity(root)?;
    agent_fixture(source, identity, false, true)?;
    assert_eq!(
        call_tool(source, "demo.echo").err().as_deref(),
        Some("EACCES: AgentPolicy")
    );
    agent_fixture(source, identity, true, false)?;
    assert_eq!(
        call_tool(source, "demo.echo").err().as_deref(),
        Some("EACCES: ToolPolicy")
    );
    agent_fixture(source, identity, true, true)?;
    let shadow = source
        .join("home")
        .join(identity.uid.to_string())
        .join("tool/demo.echo");
    fs::create_dir_all(shadow.parent().ok_or("shadow parent missing")?)?;
    fs::write(&shadow, "#!/bin/sh\nexit 99\n")?;
    let mut permissions = fs::metadata(&shadow)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&shadow, permissions)?;
    let (called, authorized) = call_tool(source, "demo.echo").map_err(io::Error::other)?;
    assert_eq!(authorized, source.join("tool/demo.echo"));
    assert!(called.status.success(), "{called:?}");
    assert_no_secret(&called.stdout);
    assert_no_secret(&called.stderr);
    let environment: Value = serde_json::from_slice(&fs::read(root.join("observed.json"))?)?;
    assert_eq!(environment.get("control"), None);
    assert_eq!(environment.get("undeclared"), Some(&Value::Null));
    let frames = cortexfs_tool_sdk::parse_jsonl_frames(&String::from_utf8(called.stdout)?)?;
    assert_eq!(
        frames
            .first()
            .and_then(|frame| frame.get("type"))
            .and_then(Value::as_str),
        Some("start")
    );
    let message = frames
        .iter()
        .find(|frame| frame.get("type").and_then(Value::as_str) == Some("message"))
        .ok_or("tool message missing")?;
    assert_eq!(
        message.get("content"),
        Some(&json!([{"type":"text","text":"remote-ok"}]))
    );
    assert_eq!(
        frames
            .last()
            .and_then(|frame| frame.get("status"))
            .and_then(Value::as_str),
        Some("ok")
    );
    fs::write(root.join("mcp.json"), b"{}")?;
    let (changed, _) = call_tool(source, "demo.echo").map_err(io::Error::other)?;
    assert!(!changed.status.success(), "changed MCP config was accepted");
    assert!(
        String::from_utf8_lossy(&changed.stdout).contains("MCP config digest mismatch"),
        "unexpected changed-config output: {changed:?}",
    );
    Ok(())
}

#[test]
fn project_install_discover_and_execute_remote_tool() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let installed = project_and_install(root.path())?;
    exercise_remote_tool(root.path(), &installed)
}
