#[test]
fn agent_new_host_fallback_defaults_worker_to_default_worker_model() {
    let root = clean_test_dir("ctx-agent-new-host-worker-default-model");
    let command = cmd!("agent", "new", "worker-fast", "--parent", "agent:coder");
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert_eq!(
        fs::read_to_string(root.join("agent/worker-fast.d/model")).unwrap_or_default(),
        "openai/gpt-5.6\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/worker-fast.d/policy")).unwrap_or_default(),
        "allow worker-fast_t model:openai/gpt-5.6 use\nallow worker-fast_t tool:tsh execute\nallow worker-fast_t network:default connect\n"
    );
}

#[test]
fn agent_new_host_fallback_defaults_executor_to_default_worker_model() {
    let root = clean_test_dir("ctx-agent-new-host-executor-default-model");
    let command = cmd!(
        "agent",
        "new",
        "executor-fast",
        "--parent",
        "agent:architect"
    );
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert_eq!(
        fs::read_to_string(root.join("agent/executor-fast.d/model")).unwrap_or_default(),
        "openai/gpt-5.6\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/executor-fast.d/policy")).unwrap_or_default(),
        "allow executor-fast_t model:openai/gpt-5.6 use\nallow executor-fast_t tool:tsh execute\nallow executor-fast_t network:default connect\n"
    );
}

#[test]
fn agent_new_host_fallback_keeps_non_worker_default_on_main() {
    let root = clean_test_dir("ctx-agent-new-host-coder-stub-default-model");
    let command = cmd!("agent", "new", "coder", "--parent", "agent:architect");
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/model")).unwrap_or_default(),
        "main\n"
    );
}

#[test]
fn agent_new_host_fallback_rejects_invalid_parent_ref() {
    let root = clean_test_dir("ctx-agent-new-host-bad-parent");
    let command = cmd!("agent", "new", "worker-fast", "--parent", "session:default");
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert!(matches!(
        agent_new_host_fallback(&root, &args),
        Err(ref error)
            if error.code == 2 && error.message == "invalid agent parent: session:default"
    ));
    assert!(!root.join("agent/worker-fast.d").exists());
    assert!(!root.join("agent/worker-fast").exists());
}

#[test]
fn agent_new_host_fallback_rejects_invalid_model_without_writing_controls() {
    let root = clean_test_dir("ctx-agent-new-host-bad-model");
    let command = cmd!(
        "agent",
        "new",
        "worker-fast",
        "--parent",
        "agent:coder",
        "--model",
        "bad/model/name"
    );
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert!(matches!(
        agent_new_host_fallback(&root, &args),
        Err(ref error)
            if error.code == 2 && error.message == "invalid model name: bad/model/name"
    ));
    assert!(!root.join("agent/worker-fast.d").exists());
    assert!(!root.join("agent/worker-fast").exists());
}

#[test]
fn agent_new_host_fallback_rejects_invalid_name_without_writing_controls() {
    let root = clean_test_dir("ctx-agent-new-host-bad-name");
    let args = AgentNewArgs {
        name: "../worker".to_owned(),
        temporary: false,
        parent: Some("agent:coder".to_owned()),
        label: None,
        models: Vec::new(),
        tools: Vec::new(),
        shared: Vec::new(),
        mounts: Vec::new(),
        instructions: None,
        description: None,
    };

    assert!(matches!(
        agent_new_host_fallback(&root, &args),
        Err(ref error) if error.code == 2 && error.message == "invalid agent name: ../worker"
    ));
    assert!(!root.join("agent/../worker.d").exists());
    assert!(!root.join("agent/../worker").exists());
}

#[test]
fn agent_new_host_fallback_rejects_invalid_mount_without_writing_controls() {
    let root = clean_test_dir("ctx-agent-new-host-bad-mount");
    let command = cmd!(
        "agent",
        "new",
        "worker-fast",
        "--parent",
        "agent:coder",
        "--mount",
        "relative:/workspace:rw"
    );
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert!(matches!(
        agent_new_host_fallback(&root, &args),
        Err(ref error) if error.code == 2 && error.message == "agent mount paths must be absolute"
    ));
    assert!(!root.join("agent/worker-fast.d").exists());
    assert!(!root.join("agent/worker-fast").exists());
}
