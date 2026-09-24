include!("parse_paths/basic_commands.rs");
include!("parse_paths/session_commands.rs");
include!("parse_paths/agent_lifecycle.rs");
include!("parse_paths/agent_lifecycle_worker_model.rs");
include!("parse_paths/agent_lifecycle_temp_cleanup.rs");
include!("parse_paths/agent_parent_validation.rs");
include!("parse_paths/agent_process.rs");
include!("parse_paths/agent_profile.rs");
include!("parse_paths/agent_status_validation.rs");
include!("parse_paths/agent_start.rs");
include!("parse_paths/chat.rs");
include!("parse_paths/agent_rendering.rs");
include!("parse_paths/tools_and_paths.rs");
include!("parse_paths/abi_detection.rs");

#[test]
fn explicit_hosted_child_disables_systemd_restart() {
    let root = clean_test_dir("ctx-agent-hosted-child-restart");
    assert!(ensure_reference_tree(&root).is_ok());
    ensure_runtime_model_fixture(&root);
    let Ok(view) = derive_agent_runtime_view(&root, "executor") else {
        return;
    };
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: false,
        mounts: Vec::new(),
        command: vec!["/workspace/fake-agent".to_owned()],
    };
    let command = agent_start_systemd_command(
        &root,
        &args,
        &[],
        &view,
        Path::new(cortexfs::runtime::terminal::broker::BROKER_SOCKET),
        "cortexfs-agent-executor-test-terminal",
    );
    assert!(!command
        .args
        .iter()
        .any(|arg| arg.starts_with("--property=Restart")));
}
