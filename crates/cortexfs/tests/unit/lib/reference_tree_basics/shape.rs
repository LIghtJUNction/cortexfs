#[test]
fn root_is_ctx() {
    assert_eq!(CTX_ROOT, "/ctx");
}

#[test]
fn root_keeps_only_short_agent_os_entries() {
    assert!(is_root_entry("model"));
    assert!(is_root_entry("agent"));
    assert!(is_root_entry("tool"));
    assert!(!is_root_entry("provider"));
    assert!(!is_root_entry("format"));
    assert!(!is_root_entry("db"));
    assert!(!is_root_entry("vector"));
    assert!(!is_root_entry("mcp"));
    assert!(!is_root_entry("cluster"));
    assert!(!is_root_entry("audit"));
    assert!(!is_root_entry("control"));
    assert!(!is_root_entry("AGENTS.rc"));
}

#[test]
fn executable_objects_are_model_agent_tool() {
    assert_eq!(EXEC_OBJECTS, ["model", "agent", "tool"]);
    assert_eq!(ObjectClass::parse("model"), Some(ObjectClass::Model));
    assert_eq!(ObjectClass::parse("agent"), Some(ObjectClass::Agent));
    assert_eq!(ObjectClass::parse("tool"), Some(ObjectClass::Tool));
    assert_eq!(ObjectClass::parse("provider"), None);
}

#[test]
fn reference_tree_socket_errors_report_specific_errno() {
    assert_eq!(
        ReferenceTreeError::CannotSocket(std::io::ErrorKind::PermissionDenied).errno(),
        "EACCES"
    );
    assert_eq!(
        ReferenceTreeError::CannotSocket(std::io::ErrorKind::AlreadyExists).errno(),
        "EEXIST"
    );
}

#[test]
fn object_names_are_small_ascii_path_components() {
    assert!(is_object_name("echo"));
    assert!(is_object_name("fs.read"));
    assert!(is_object_name("mcp.github.search_issues"));
    assert!(is_object_name("agent_1+dev-2"));
    assert!(is_object_name(&"a".repeat(MAX_OBJECT_NAME_LEN)));

    assert!(!is_object_name(""));
    assert!(!is_object_name("."));
    assert!(!is_object_name(".."));
    assert!(!is_object_name("-bad"));
    assert!(!is_object_name("_bad"));
    assert!(!is_object_name("bad/name"));
    assert!(!is_object_name("bad\nname"));
    assert!(!is_object_name("echo.sock"));
    assert!(!is_object_name("echo.d"));
    assert!(!is_object_name("中文"));
    assert!(!is_object_name(&"a".repeat(MAX_OBJECT_NAME_LEN + 1)));
}

#[test]
fn abi_paths_classify_by_stable_shape() {
    for model in [
        "openai/gpt-4o",
        "openai/gpt-4.1",
        "anthropic/claude-sonnet-4",
        "google/gemini-2.5-pro",
        "meta-llama/llama-4-maverick",
        "x-ai/grok-4",
    ] {
        assert_abi_class(&format!("model/{model}"), "ctx.model.exec");
    }
    for (path, expected) in [
        ("model/route", "ctx.model.route"),
        ("model/debug/echo", "ctx.model.exec"),
        ("model/debug/echo.sock", "ctx.model.socket"),
        ("model/debug/echo.d/id", "ctx.model.control"),
        ("agent/coder", "ctx.agent.exec"),
        ("agent/coder.sock", "ctx.agent.socket"),
        ("agent/coder.d/policy", "ctx.agent.control"),
        ("tool/fs.read", "ctx.tool.exec"),
        ("tool/fs.read.d/schema", "ctx.tool.control"),
        ("home/1000", "ctx.home.dir"),
        ("home/1000/agent/coder/session/default", "ctx.session.dir"),
        (
            "home/1000/agent/coder/session/default/messages.jsonl",
            "ctx.session.messages",
        ),
        (
            "home/1000/agent/coder/session/default/events.jsonl",
            "ctx.session.events",
        ),
        (
            "home/1000/model/debug/echo.d/session/default",
            "ctx.session.dir",
        ),
        (
            "shared/im-qq-dev/agent/bot/session/group-456/events.jsonl",
            "ctx.session.events",
        ),
        (
            "shared/project-a/model/debug/echo.d/session/default/messages.jsonl",
            "ctx.session.messages",
        ),
        ("shared/project-a", "ctx.shared.dir"),
        ("shared/project-a/tool/project.test", "ctx.shared.tool.exec"),
        (
            "shared/project-a/tool/project.test.d/schema",
            "ctx.shared.tool.control",
        ),
        ("shared/project-a/queue", "ctx.shared.queue"),
        ("shared/project-a/queue/pending", "ctx.shared.queue"),
        ("shared/project-a/result", "ctx.shared.result"),
    ] {
        assert_abi_class(path, expected);
    }
}

#[test]
fn abi_path_classifier_rejects_forbidden_root_and_bad_names() {
    for path in [
        "provider/openai",
        "mcp/github",
        "skill/local",
        "cluster/default",
        "model/debug/echo.sock.d/id",
        "tool/-bad",
        "agent/coder/extra",
    ] {
        assert_abi_class(path, "ctx.unknown");
    }
}

#[test]
fn reference_tree_bootstrap_materializes_documented_v1_shape() {
    let root = clean_test_dir("reference-tree");
    let user_tool_dir = ctx_home(&root).join("tool");
    for tool in [
        "mcp.github.search_issues",
        "agent.create",
        "agent.start",
        "agent.stop",
    ] {
        assert!(install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            tool,
            "/bin/false",
            &[("description", "CortexFS reference-tree tool")],
        )
        .is_ok());
    }
    assert!(fs::create_dir_all(&user_tool_dir).is_ok());
    assert!(symlink(
        Path::new("/ctx/tool/fs.read"),
        user_tool_dir.join("fs.read")
    )
    .is_ok());

    let bootstrapped = ensure_v1_reference_tree(&root);
    let bootstrapped = ok!(bootstrapped);
    assert_eq!(bootstrapped.root(), root.as_path());

    assert_file_text(&root.join("status"), "ready\n");
    let status_mode = fs::metadata(root.join("status"))
        .map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(status_mode, Ok(0o644)));
    assert!(root.join("bin").join("ctx").is_file());
    assert!(root.join("bin").join("ctxterm").is_file());
    assert!(!root.join("bin").join("te").exists());
    assert!(root.join("bin").join("tsh").is_file());
    assert_reference_bin_placeholders(&root);
    assert!(!root.join("model").join("debug").join("echo").exists());
    let agent_socket_mode = fs::metadata(root.join("agent").join("coder.sock"))
        .map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(agent_socket_mode, Ok(0o777)));
    assert!(!root.join("mcp").exists());
    assert!(!root.join("skill").exists());
    assert!(!root.join("memory").exists());
    assert_file_text(
        &root.join("home").join("1000").join(".tshrc"),
        "CTX_PATH=/ctx/tool:/ctx/home/1000/tool\n",
    );

    assert!(inspect_object_layout(&root, ObjectClass::Model, "debug/echo").is_ok());
    assert_reference_agents(&root);
    assert!(inspect_object_layout(&root, ObjectClass::Tool, "tsh").is_ok());
    for tool in ["bash", "tmux", "zellij", "fs.read", "fs.write", "shell.exec"] {
        assert!(!root.join("tool").join(tool).exists());
        assert!(!root.join("tool").join(format!("{tool}.d")).exists());
    }
    for tool in [
        "mcp.github.search_issues",
        "agent.create",
        "agent.start",
        "agent.stop",
    ] {
        assert!(!root.join("tool").join(tool).exists());
        assert!(!root.join("tool").join(format!("{tool}.d")).exists());
    }

    let schema = fs::read_to_string(root.join("tool").join("tsh.d/schema"));
    let schema = ok!(schema);
    assert!(inspect_tool_schema_json(&schema).is_ok());

    let private_session_root = agent_session_root(&root, "coder");
    for index in ["by-cwd", "by-hash", "by-uuid"] {
        assert!(private_session_root.join("index").join(index).is_dir());
    }
    assert!(!private_session_root.join("default").exists());

    assert!(user_tool_dir.is_dir());
    assert!(!user_tool_dir.join("fs.read").exists());
    let model_link = fs::read_link(ctx_home(&root).join("model").join("coder"));
    assert!(matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/main")));

    assert!(root.join("shared").is_dir());
    assert!(!root.join("shared").join("project-a").exists());

    assert_eq!(ensure_v1_reference_tree(&root), Ok(bootstrapped));
}

fn assert_reference_agents(root: &Path) {
    for agent in ["base", "coder", "reviewer", "executor", "worker"] {
        assert!(inspect_object_layout(root, ObjectClass::Agent, agent).is_ok());
    }
    assert_file_text(&root.join("agent").join("base.d").join("parent"), "\n");
    assert_file_text(&root.join("agent").join("base.d").join("cwd"), "/workspace\n");
    assert_file_text(&root.join("agent").join("coder.d").join("cwd"), "/workspace\n");
    assert_file_text(&root.join("agent").join("coder.d").join("model"), "main\n");
    assert_file_text(&root.join("agent").join("reviewer.d").join("model"), "helper\n");
    let coder_system = ok!(fs::read_to_string(
        root.join("agent").join("coder.d").join("system.md")
    ));
    assert!(coder_system.contains("prefer a delegated `react` node"));
    assert!(coder_system.contains("the omitted delegated agent is `worker`"));
    assert!(coder_system.contains("`worker-*`, `executor`, or `executor-*`"));
    assert!(coder_system.contains("shared reusable entries"));
    assert!(coder_system.contains("dedicated temp workers"));
    assert!(coder_system.contains("`model=`, `life=`, `plan=`, `handoff=`, `result=`, and `refs=`"));
    assert!(coder_system.contains("ctx agent wait"));
    assert_file_text(
        &root.join("agent").join("executor.d").join("model"),
        "api.lmm.best/gpt-5.3-codex-spark\n",
    );
    assert_file_text(
        &root.join("agent").join("executor.d").join("parent"),
        "agent:base\n",
    );
    assert_file_text(
        &root.join("agent").join("worker.d").join("model"),
        "api.lmm.best/gpt-5.3-codex-spark\n",
    );
    assert_file_text(
        &root.join("agent").join("worker.d").join("parent"),
        "agent:coder\n",
    );
    let worker_system = ok!(fs::read_to_string(
        root.join("agent").join("worker.d").join("system.md")
    ));
    assert!(worker_system.contains("spark model path"));
    assert!(worker_system.contains("Worker-role agent names include"));
    assert!(worker_system.contains("preserve its `model=` and `life=` context"));
    assert!(worker_system.contains("bounded delegated implementation tasks"));
    assert!(worker_system.contains("Do not make architecture decisions"));
    for agent in ["coder", "reviewer"] {
        assert_file_text(
            &root.join("agent").join(format!("{agent}.d")).join("parent"),
            "agent:base\n",
        );
    }

    let base_policy = ok!(fs::read_to_string(
        root.join("agent").join("base.d").join("policy")
    ));
    assert!(base_policy.contains("allow base_t tool:tsh execute\n"));
    assert!(base_policy.contains("allow base_t tool:fs.read execute\n"));
    assert!(base_policy.contains("allow base_t network:default connect\n"));
    for child in ["coder", "reviewer", "executor"] {
        assert!(base_policy.contains(&format!("allow base_t agent:{child} create\n")));
        assert!(base_policy.contains(&format!("allow base_t agent:{child} start\n")));
    }
    let coder_policy = ok!(fs::read_to_string(
        root.join("agent").join("coder.d").join("policy")
    ));
    for permission in ["create", "start", "stop", "read"] {
        assert!(coder_policy.contains(&format!("allow coder_t agent:worker {permission}\n")));
    }
    for agent in ["base", "executor", "worker"] {
        for index in ["by-cwd", "by-hash", "by-uuid"] {
            assert!(agent_session_root(root, agent)
                .join("index")
                .join(index)
                .is_dir());
        }
    }
}

#[test]
fn reference_tree_bootstrap_does_not_chmod_socket_symlink_target() {
    let root = clean_test_dir("reference-tree-socket-symlink-mode");
    let outside = clean_test_dir("reference-tree-socket-symlink-mode-outside");
    assert!(fs::create_dir_all(&outside).is_ok());
    let outside_socket = outside.join("runtime.sock");
    let listener = ok!(UnixListener::bind(&outside_socket));
    set_file_mode(&outside_socket, 0o600);
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    assert!(symlink(&outside_socket, root.join("agent").join("coder.sock")).is_ok());

    let bootstrapped = ensure_v1_reference_tree(&root);

    assert!(matches!(
        bootstrapped,
        Err(ReferenceTreeError::CannotSocket(
            std::io::ErrorKind::AlreadyExists
        ))
    ));
    assert!(root
        .join("agent")
        .join("coder.sock")
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink()));
    let target_mode =
        fs::metadata(&outside_socket).map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(target_mode, Ok(0o600)));
    drop(listener);
    let _ignored = fs::remove_dir_all(outside);
}
