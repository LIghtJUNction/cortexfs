include!("parse_paths/basic_commands.rs");
include!("parse_paths/session_commands.rs");
include!("parse_paths/agent_lifecycle.rs");
include!("parse_paths/agent_lifecycle_worker_model.rs");
include!("parse_paths/agent_lifecycle_temp_cleanup.rs");
include!("parse_paths/agent_parent_validation.rs");
include!("parse_paths/agent_process.rs");
include!("parse_paths/agent_profile.rs");
include!("parse_paths/agent_status_validation.rs");
#[allow(clippy::panic, clippy::indexing_slicing)]
include!("parse_paths/agent_start.rs");
include!("parse_paths/chat.rs");
include!("parse_paths/agent_rendering.rs");
include!("parse_paths/tools_and_paths.rs");
include!("parse_paths/abi_detection.rs");

#[test]
fn writable_workspace_bwrap_does_not_invent_git_mask() {
    let source = clean_test_dir("ctx-agent-git-bwrap-args-source");
    assert!(fs::create_dir_all(source.join(".git")).is_ok());
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
        command: Vec::new(),
    };
    let mounts = agent_start_mounts_with_default_source(&args, &source);
    let Some(bwrap) = agent_bwrap_test_args(&args, &mounts) else {
        return;
    };
    assert!(contains_arg_triplet(
        &bwrap,
        "--bind",
        source.to_str().unwrap_or_default(),
        "/workspace"
    ));
    assert!(!contains_arg_pair(&bwrap, "--tmpfs", "/workspace/.git"));
    assert!(!contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        "/dev/null",
        "/workspace/.git"
    ));
}
