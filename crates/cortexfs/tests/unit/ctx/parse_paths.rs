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