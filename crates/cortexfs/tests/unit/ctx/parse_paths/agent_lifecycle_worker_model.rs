#[test]
fn agent_new_host_fallback_defaults_worker_to_spark_model() {
    let root = clean_test_dir("ctx-agent-new-host-worker-default-model");
    let command = cmd!("agent", "new", "worker-fast", "--parent", "agent:coder");
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert_eq!(
        fs::read_to_string(root.join("agent/worker-fast.d/model")).unwrap_or_default(),
        "api.lmm.best/gpt-5.3-codex-spark\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker-fast.d/policy")).unwrap_or_default(),
        "allow worker-fast_t model:api.lmm.best/gpt-5.3-codex-spark use\nallow worker-fast_t tool:tsh execute\nallow worker-fast_t network:default connect\n"
    );
    assert!(
        fs::read_to_string(root.join("agent/worker-fast"))
            .unwrap_or_default()
            .contains("model=\"api.lmm.best/gpt-5.3-codex-spark\"")
    );
}

#[test]
fn agent_new_host_fallback_defaults_executor_to_spark_model() {
    let root = clean_test_dir("ctx-agent-new-host-executor-default-model");
    let command = cmd!("agent", "new", "executor-fast", "--parent", "agent:base");
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert_eq!(
        fs::read_to_string(root.join("agent/executor-fast.d/model")).unwrap_or_default(),
        "api.lmm.best/gpt-5.3-codex-spark\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/executor-fast.d/policy")).unwrap_or_default(),
        "allow executor-fast_t model:api.lmm.best/gpt-5.3-codex-spark use\nallow executor-fast_t tool:tsh execute\nallow executor-fast_t network:default connect\n"
    );
}

#[test]
fn agent_new_host_fallback_keeps_non_worker_stub_default_on_main() {
    let root = clean_test_dir("ctx-agent-new-host-coder-stub-default-model");
    let command = cmd!("agent", "new", "coder", "--parent", "agent:base");
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert!(
        fs::read_to_string(root.join("agent/coder"))
            .unwrap_or_default()
            .contains("model=\"main\"")
    );
}
