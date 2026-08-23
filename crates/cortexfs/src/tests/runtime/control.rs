#[test]
fn run_control_is_consumed_only_by_create_and_update() {
    assert!(crate::runtime::control::consumes_run_control(
        "agent.create"
    ));
    assert!(crate::runtime::control::consumes_run_control(
        "agent.update"
    ));
    assert!(!crate::runtime::control::consumes_run_control("tsh"));
    assert!(!crate::runtime::control::consumes_run_control("fs.read"));
    assert!(!crate::runtime::control::consumes_run_control("probe"));
    assert!(!crate::runtime::control::consumes_run_control("agent.stop"));
}
