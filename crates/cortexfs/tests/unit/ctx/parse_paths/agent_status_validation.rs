#[test]
fn agent_status_rejects_live_child_with_invalid_model() {
    let root = clean_test_dir("ctx-agent-status-child-invalid-model");
    create_agent_fixture(&root, "executor", "agent:base", "ready", "");
    create_agent_fixture(&root, "worker", "agent:executor", "ready", "");
    write_text_file(&root.join("agent/worker.d/model"), "bad/model/name\n");

    assert!(matches!(
        agent_status_lines(&root, "executor"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid agent model for worker: bad/model/name"
    ));
}

#[test]
fn agent_status_rejects_live_child_with_invalid_lifecycle() {
    let root = clean_test_dir("ctx-agent-status-child-invalid-life");
    create_agent_fixture(&root, "executor", "agent:base", "ready", "");
    create_agent_fixture(&root, "worker", "agent:executor", "ready", "");
    write_text_file(&root.join("agent/worker.d/life"), "detached\n");

    assert!(matches!(
        agent_status_lines(&root, "executor"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid agent life for worker: detached"
    ));
}
