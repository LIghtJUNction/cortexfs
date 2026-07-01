#[test]
fn agent_stop_host_fallback_treats_missing_child_life_as_owned() {
    let root = clean_test_dir("ctx-agent-stop-missing-child-life");
    create_agent_fixture(&root, "coder", "agent:base", "busy", "100");
    create_agent_fixture(&root, "worker", "agent:coder session:default", "busy", "101");
    write_text_file(&root.join("agent/coder.d/log"), "");
    write_text_file(&root.join("agent/worker.d/log"), "");
    assert!(fs::remove_file(root.join("agent/worker.d/life")).is_ok());

    assert_eq!(agent_stop(&root, "coder"), Ok(ExitCode::SUCCESS));

    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/status")).unwrap_or_default(),
        "dead\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker.d/pid")).unwrap_or_default(),
        "\n"
    );
    assert!(root.join("agent/worker.d").is_dir());
}
