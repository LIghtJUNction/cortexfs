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
    assert_eq!(command.args.windows(3).filter(|w| w.first().is_some_and(|v| v == "--bind") && w.get(2).is_some_and(|v| v == "/ctx")).count(), 1);
    assert!(agent_bwrap_test_args(&args, &[]).is_some() && command.args.iter().any(|arg| arg.contains("ExecCondition=/usr/bin/findmnt") && arg.contains("--source cortexfs") && arg.contains("--options rw")));
    let host_ready = cortexfs::is_executable_file(Path::new(cortexfs::support::command::PASTA)) && Path::new("/dev/net/tun").exists();
    let order = [cortexfs::support::command::BWRAP, cortexfs::support::command::PASTA, cortexfs::support::command::CTXTERM].map(|arg| command.args.iter().position(|value| value == arg));
    assert!(!host_ready || matches!(order, [Some(bwrap), Some(pasta), Some(ctxterm)] if bwrap < pasta && pasta < ctxterm));
    assert!(!host_ready || (command.args.iter().any(|arg| arg == cortexfs::support::command::PASTA) && command.args.iter().any(|arg| arg == "--no-map-gw") && !command.args.iter().any(|arg| arg == "--unshare-net") && command.args.iter().any(|arg| arg == "/dev/net/tun")));
    assert!((host_ready && [["-t", "none"], ["-u", "none"], ["-T", "none"], ["-U", "none"]].iter().all(|pair| command.args.windows(2).any(|w| w == pair))) || (!host_ready && command.args.iter().any(|arg| arg == "--unshare-net") && !command.args.iter().any(|arg| arg == cortexfs::support::command::PASTA)));
    write_text_file(&root.join("agent/executor.d/policy"), "allow executor_t tool:tsh execute\n");
    let Ok(denied_view) = derive_agent_runtime_view(&root, "executor") else { return };
    let denied = agent_start_systemd_command(Path::new("/ctx"), &args, &[], &denied_view, Path::new(cortexfs::runtime::terminal::broker::BROKER_SOCKET), "ctx-agent-network-denied-test");
    assert!(denied.args.iter().any(|arg| arg == "--unshare-net") && !denied.args.iter().any(|arg| arg == cortexfs::support::command::PASTA));
}
