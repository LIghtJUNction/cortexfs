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
fn agent_start_keeps_one_ctx_root_projection() {
    let Ok(Command::Agent(AgentArgs::Start(args))) = cmd!("agent", "start", "executor") else {
        return;
    };
    let bwrap = agent_bwrap_test_args(&args, &[]).unwrap_or_default();
    assert_eq!(
        bwrap
            .windows(3)
            .filter(|window| {
                matches!(window.first().map(String::as_str), Some("--bind" | "--ro-bind"))
                    && window.get(2).is_some_and(|target| target == "/ctx")
            })
            .count(),
        1
    );
}

#[test]
fn hosted_agent_guards_writable_system_ctx_at_unit_start() {
    let root = clean_test_dir("ctx-agent-rw-root-guard");
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
        command: vec!["/bin/true".to_owned()],
    };
    let command = agent_start_systemd_command(
        Path::new("/ctx"),
        &args,
        &[],
        &view,
        Path::new(cortexfs::runtime::terminal::broker::BROKER_SOCKET),
        "cortexfs-agent-executor-test-terminal",
    );
    assert!(contains_arg_triplet(&command.args, "--bind", "/ctx", "/ctx"));
    assert!(command.args.iter().any(|arg| {
        arg.starts_with("--property=ExecCondition=/usr/bin/findmnt ")
            && arg.contains("--mountpoint /ctx")
            && arg.contains("--source cortexfs")
            && arg.contains("--options rw")
    }));

    let fallback = agent_start_systemd_command(
        &root,
        &args,
        &[],
        &view,
        Path::new(cortexfs::runtime::terminal::broker::BROKER_SOCKET),
        "cortexfs-agent-executor-test-terminal",
    );
    assert!(contains_arg_triplet(
        &fallback.args,
        "--ro-bind",
        &root.display().to_string(),
        "/ctx"
    ));
    assert!(!fallback
        .args
        .iter()
        .any(|arg| arg.contains("ExecCondition=/usr/bin/findmnt")));
}
