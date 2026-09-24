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
    let Ok(Command::Agent(AgentArgs::Start(args))) =
        parse_agent_command(vec!["start".into(), "executor".into()])
    else {
        return;
    };
    let Some(bwrap) = agent_bwrap_test_args(&args, &[]) else {
        return;
    };
    assert_eq!(
        bwrap
            .windows(3)
            .filter(|window| {
                matches!(window[0].as_str(), "--bind" | "--ro-bind") && window[2] == "/ctx"
            })
            .count(),
        1
    );
}
