use super::*;
use std::os::unix::fs::PermissionsExt;
use std::process::ExitCode;

const UPDATE_CHILD: &str = "CORTEXFS_TSH_UPDATE_CHILD";

#[test]
fn tsh_forwards_run_control_socket_to_agent_update() {
    if std::env::var_os(UPDATE_CHILD).is_none() {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-tsh-update-control-{}",
            std::process::id()
        ));
        let output = std::process::Command::new(std::env::current_exe().unwrap_or_default())
            .arg("--exact")
            .arg("tests::control::tsh_forwards_run_control_socket_to_agent_update")
            .arg("--nocapture")
            .env(UPDATE_CHILD, "1")
            .env("CORTEXFS_TSH_ROOT", &root)
            .env("CTX_AGENT", "executor")
            .env("CTX_SESSION", "live")
            .env("CTX_RUN_ID", "run-1")
            .env("CTX_SOURCE", &root)
            .env("CTX_CONTROL_SOCKET", "/run/cortexfs/control.sock")
            .output();
        assert!(matches!(output, Ok(ref output) if output.status.success()));
        let _ignored = fs::remove_dir_all(root);
        return;
    }

    let root = std::env::var_os("CORTEXFS_TSH_ROOT")
        .map(PathBuf::from)
        .unwrap_or_default();
    write_coder_update_fixture(&root);
    let result = run_tool(&root, "agent.update", Vec::new());
    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
}

fn write_coder_update_fixture(root: &Path) {
    let control = root.join("agent").join("executor.d");
    let tool_control = root.join("tool").join("agent.update.d");
    assert!(fs::create_dir_all(&control).is_ok());
    assert!(fs::create_dir_all(&tool_control).is_ok());
    for (file, value) in [
        ("abi", "sdk-envelope-v1\n"),
        ("owner", "1000\n"),
        ("uid", "1000\n"),
        ("gid", "1000\n"),
        ("groups", "1000\n"),
        ("perm", "rwx\n"),
        ("label", "user_u:agent_r:executor_t:s0\n"),
        ("iso", "shared\n"),
        ("parent", "\n"),
        ("life", "owned\n"),
        ("root", "/ctx/home/1000/agent/executor/root\n"),
        ("cwd", "/workspace\n"),
        ("env", "\n"),
        ("model", "main\n"),
        ("window", "auto\n"),
        ("status", "idle\n"),
        ("pid", "\n"),
        ("log", "\n"),
        ("meta.json", "{}\n"),
    ] {
        assert!(fs::write(control.join(file), value).is_ok());
    }
    let model_control = root.join("model/local/chat.d");
    assert!(fs::create_dir_all(&model_control).is_ok());
    assert!(fs::write(model_control.join("limit"), "unknown\n").is_ok());
    assert!(std::os::unix::fs::symlink("/ctx/model/local/chat", root.join("model/main")).is_ok());
    let session = root.join("home/1000/agent/executor/session/live");
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::write(session.join("current_run"), "run-1\n").is_ok());
    assert!(
        fs::write(
            control.join("path"),
            format!("{}\n", root.join("tool").display())
        )
        .is_ok()
    );
    assert!(
        fs::write(
            control.join("mount"),
            format!(
                "{}\t{}\tro\trbind,nosuid,nodev\n",
                root.display(),
                root.display()
            ),
        )
        .is_ok()
    );
    let policy = "allow executor_t model:main use\nallow executor_t tool:agent.update execute\n";
    assert!(fs::write(control.join("policy"), policy).is_ok());
    assert!(fs::write(tool_control.join("policy"), policy).is_ok());
    let tool = root.join("tool").join("agent.update");
    assert!(
        fs::write(
            &tool,
            r#"#!/bin/sh
[ "$CTX_CONTROL_SOCKET" = /run/cortexfs/control.sock ] || exit 19
exit 0
"#,
        )
        .is_ok()
    );
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
}
