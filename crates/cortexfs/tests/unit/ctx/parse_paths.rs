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
fn hosted_agent_guards_one_writable_system_ctx_projection() {
    let root = clean_test_dir("ctx-agent-rw-root-guard");
    assert!(ensure_reference_tree(&root).is_ok());
    ensure_runtime_model_fixture(&root);
    let Ok(view) = derive_agent_runtime_view(&root, "executor") else { return };
    let Ok(Command::Agent(AgentArgs::Start(mut args))) = cmd!("agent", "start", "executor") else { return };
    args.command = vec!["/bin/true".to_owned()];
    let command = agent_start_systemd_command(
        Path::new("/ctx"), &args, &[], &view,
        Path::new(cortexfs::runtime::terminal::broker::BROKER_SOCKET), "ctx-agent-rw-test",
    );
    assert_eq!(command.args.windows(3).filter(|w| w[0] == "--bind" && w[2] == "/ctx").count(), 1);
    assert!(command.args.iter().any(|arg| arg.contains("ExecCondition=/usr/bin/findmnt") && arg.contains("--source cortexfs") && arg.contains("--options rw")));
    assert!(command.args.iter().any(|arg| arg == cortexfs::support::command::PASTA) && contains_arg_pair(&command.args, "--map-guest-addr", "none") && !command.args.iter().any(|arg| arg == "--unshare-net"));
}
