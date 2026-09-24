#[test]
fn agent_status_rejects_invalid_live_child_facts() {
    for (fact, value, message) in [
        ("model", "bad/model/name", "invalid agent model for worker: bad/model/name"),
        ("life", "detached", "invalid agent life for worker: detached"),
    ] {
        let root = clean_test_dir(&format!("ctx-agent-status-child-invalid-{fact}"));
        create_agent_fixture(&root, "executor", "agent:base", "ready", "");
        create_agent_fixture(&root, "worker", "agent:executor", "ready", "");
        write_text_file(&root.join(format!("agent/worker.d/{fact}")), &format!("{value}\n"));
        assert!(matches!(
            agent_status_lines(&root, "executor"),
            Err(ref error) if error.code == 2 && error.message == message
        ));
    }
}
