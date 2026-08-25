use crate::object::runner::requests::current_agent_openai_tools_for;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
#[expect(
    clippy::expect_used,
    reason = "test fixture setup should fail loudly with the missing field name"
)]
fn declared_tools_extend_openai_tool_manifest_and_cache_does_not() {
    let root = temp_path("provider-loaded-tools");
    let _ignored = fs::remove_dir_all(&root);
    let control = root.join("agent").join("executor.d");
    fs::create_dir_all(&control).expect("agent control");
    fs::write(control.join("owner"), "1000\n").expect("owner");
    fs::write(control.join("uid"), "1000\n").expect("uid");
    fs::write(control.join("gid"), "1000\n").expect("gid");
    fs::write(control.join("groups"), "1000\n").expect("groups");
    fs::write(control.join("perm"), "rwx\n").expect("perm");
    fs::write(control.join("label"), "user_u:agent_r:executor_t:s0\n").expect("label");
    fs::write(control.join("iso"), "shared\n").expect("iso");
    fs::write(control.join("parent"), "\n").expect("parent");
    fs::write(control.join("life"), "owned\n").expect("life");
    fs::write(control.join("root"), "/ctx/home/1000/agent/executor/root\n").expect("root");
    fs::write(control.join("cwd"), "/workspace\n").expect("cwd");
    fs::write(control.join("env"), "\n").expect("env");
    fs::write(
        control.join("path"),
        format!("{}\n", root.join("tool").display()),
    )
    .expect("path");
    fs::write(
        control.join("mount"),
        format!(
            "{}\t{}\tro\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    )
    .expect("mount");
    fs::write(control.join("model"), "local/chat\n").expect("model");
    fs::write(control.join("abi"), "sdk-envelope-v1\n").expect("abi");
    fs::write(control.join("window"), "auto\n").expect("window");
    let model_control = root.join("model/local/chat.d");
    fs::create_dir_all(&model_control).expect("model control");
    fs::write(model_control.join("limit"), "unknown\n").expect("limit");
    fs::write(
        control.join("policy"),
        "allow executor_t model:local/chat use\n",
    )
    .expect("policy");
    fs::write(control.join("tools"), "bash\nshell.exec\n").expect("tools");
    fs::write(control.join("status"), "idle\n").expect("status");
    fs::write(control.join("pid"), "\n").expect("pid");
    fs::write(control.join("log"), "\n").expect("log");
    fs::write(control.join("meta.json"), "{}\n").expect("meta");
    let view = crate::derive_agent_runtime_view(&root, "executor").expect("view");
    let mut state = crate::TshContextState::default();
    state.tools = vec![
        crate::TshLoadedToolState {
            name: "cached_only".to_owned(),
            path: root.join("tool/cached_only"),
            description: String::new(),
            schema: None,
            dynamic_resident: true,
            pinned: true,
            last_used: 1,
        },
        crate::TshLoadedToolState {
            name: "shell.exec".to_owned(),
            path: root.join("tool/shell.exec"),
            description: String::new(),
            schema: None,
            dynamic_resident: true,
            pinned: true,
            last_used: 2,
        },
    ];
    crate::write_tsh_context_state(
        &crate::tsh_context_state_path(&view.home().join("session/session-a")),
        &state,
    )
    .expect("state");
    assert_eq!(
        current_agent_openai_tools_for(Some("executor"), &root),
        vec!["bash".to_owned(), "tsh".to_owned()]
    );
    let _ignored = fs::remove_dir_all(root);
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cortexfs-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}
