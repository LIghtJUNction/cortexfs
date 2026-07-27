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
    // Keep regression coverage for legacy placeholder-style tool names in object-name validation.
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
        "openai/gpt-5.6",
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
        ("agent/coder.d/hooks/pre.d", "ctx.agent.control"),
        ("agent/coder.d/hooks/post.d", "ctx.agent.control"),
        ("tool/fs.read", "ctx.tool.exec"),
        ("tool/fs.read.d/schema", "ctx.tool.control"),
        ("tool/fs.read.d/hooks/pre.d", "ctx.tool.control"),
        ("tool/fs.read.d/hooks/post.d", "ctx.tool.control"),
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
fn reference_tree_bootstrap_materializes_documented_shape() {
    let root = clean_test_dir("reference-tree");
    let user_tool_dir = ctx_home(&root).join("tool");
    // Regression coverage for legacy placeholder-style tool names in
    // compatibility-era reference-tree layouts.
    for tool in ["mcp.github.search_issues", "agent.start", "agent.stop"] {
        assert!(
            install_executable_object_wrapper(
                &root,
                ObjectClass::Tool,
                tool,
                "/bin/false",
                &[("description", "CortexFS reference-tree tool")],
            )
            .is_ok()
        );
    }
    assert!(fs::create_dir_all(&user_tool_dir).is_ok());
    assert!(
        symlink(
            Path::new("/ctx/tool/fs.read"),
            user_tool_dir.join("fs.read")
        )
        .is_ok()
    );

    let bootstrapped = ensure_reference_tree(&root);
    let bootstrapped = ok!(bootstrapped);
    assert_eq!(bootstrapped.root(), root.as_path());

    assert_file_text(&root.join("status"), "ready\n");
    let status_mode =
        fs::metadata(root.join("status")).map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(status_mode, Ok(0o644)));
    assert!(root.join("bin").join("ctx").is_file());
    assert!(root.join("bin").join("ctxterm").is_file());
    assert!(!root.join("bin").join("te").exists());
    assert!(root.join("bin").join("tsh").is_file());
    assert_reference_bin_placeholders(&root);
    assert_file_text(
        &root.join("agent").join("coder"),
        "#!/bin/sh\n# CortexFS generated object wrapper.\n# cortexfs.object=agent\n# cortexfs.name=coder\nexec '/ctx/bin/cortexfs-object-runner' \"$0\" \"$@\"\n",
    );
    assert!(!root.join("model").join("debug").join("echo").exists());
    let agent_socket_mode = fs::metadata(root.join("agent").join("coder.sock"))
        .map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(agent_socket_mode, Ok(0o777)));
    if nix::unistd::Uid::effective().is_root() {
        assert!(matches!(
            fs::symlink_metadata(root.join("agent").join("coder.sock")),
            Ok(metadata)
                if metadata.uid() == 1000 && metadata.gid() == 1000
        ));
    }
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
    assert_object_hook_dirs(&root.join("tool").join("tsh.d"));
    assert_file_text(
        &root.join("tool").join("tsh"),
        "#!/bin/sh\n# CortexFS reference-tree tsh tool.\nexec /ctx/bin/tsh \"$@\"\n",
    );
    for tool in [
        "fs.read",
        "fs.write",
        "shell.exec",
        "tsh.config",
        "agent.create",
        "agent.update",
    ] {
        assert!(root.join("tool").join(tool).is_file());
        assert!(root.join("tool").join(format!("{tool}.d")).is_dir());
        assert!(inspect_object_layout(&root, ObjectClass::Tool, tool).is_ok());
        assert_object_hook_dirs(&root.join("tool").join(format!("{tool}.d")));
        let wrapper = fs::read_to_string(root.join("tool").join(tool)).unwrap_or_default();
        assert!(wrapper.contains("exec '/ctx/bin/cortexfs-object-runner' \"$0\" \"$@\""));
    }
    for tool in ["bash", "tmux", "zellij"] {
        assert!(!root.join("tool").join(tool).exists());
        assert!(!root.join("tool").join(format!("{tool}.d")).exists());
    }
    // Bootstrap does not delete pre-existing tools without durable ownership proof.
    for tool in ["mcp.github.search_issues", "agent.start", "agent.stop"] {
        assert!(root.join("tool").join(tool).is_file());
        assert!(root.join("tool").join(format!("{tool}.d")).is_dir());
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
    assert!(matches!(
        fs::read_link(user_tool_dir.join("fs.read")),
        Ok(ref target) if target == Path::new("/ctx/tool/fs.read")
    ));
    let user_model_dir = ctx_home(&root).join("model");
    assert!(user_model_dir.is_dir());
    assert!(!user_model_dir.join("coder").exists());

    assert!(root.join("shared").is_dir());
    assert!(!root.join("shared").join("project-a").exists());

    assert_eq!(ensure_reference_tree(&root), Ok(bootstrapped));
}

fn assert_reference_agents(root: &Path) {
    for agent in ["architect", "coder", "reviewer", "worker"] {
        assert!(inspect_object_layout(root, ObjectClass::Agent, agent).is_ok());
        assert_object_hook_dirs(&root.join("agent").join(format!("{agent}.d")));
    }
    for old_agent in ["base", "executor"] {
        assert!(!root.join("agent").join(old_agent).exists());
        assert!(!root.join("agent").join(format!("{old_agent}.d")).exists());
    }

    assert_file_text(&root.join("agent/architect.d/parent"), "\n");
    assert_file_text(&root.join("agent/architect.d/cwd"), "/workspace\n");
    assert_file_text(&root.join("agent/coder.d/parent"), "agent:architect\n");
    assert_file_text(&root.join("agent/coder.d/cwd"), "/workspace\n");
    assert_file_text(&root.join("agent/coder.d/model"), "main\n");
    assert_file_text(&root.join("agent/reviewer.d/parent"), "agent:architect\n");
    assert_file_text(&root.join("agent/reviewer.d/model"), "main\n");
    assert_file_text(&root.join("agent/worker.d/parent"), "agent:architect\n");
    assert_file_text(
        &root.join("agent/worker.d/model"),
        &format!("{DEFAULT_WORKER_MODEL}\n"),
    );

    let architect_system = ok!(fs::read_to_string(root.join("agent/architect.d/system.md")));
    assert!(architect_system.contains("human role name is Architect"));
    assert!(architect_system.contains("delegate implementation to `coder`"));
    assert!(architect_system.contains("verification to `reviewer`"));

    let coder_system = ok!(fs::read_to_string(root.join("agent/coder.d/system.md")));
    assert!(coder_system.contains("default Architect -> coder/reviewer flow"));
    assert!(coder_system.contains("fs.write"));
    assert!(coder_system.contains("shell.exec"));
    assert!(coder_system.contains("Leave architecture decisions"));

    let reviewer_system = ok!(fs::read_to_string(root.join("agent/reviewer.d/system.md")));
    assert!(reviewer_system.contains("independent review agent"));

    let worker_system = ok!(fs::read_to_string(root.join("agent/worker.d/system.md")));
    assert!(!worker_system.contains("executor"));
    assert!(reviewer_system.contains("correctness, ABI drift"));

    let architect_policy = ok!(fs::read_to_string(root.join("agent/architect.d/policy")));
    assert!(architect_policy.contains("allow architect_t agent:coder create"));
    assert!(architect_policy.contains("allow architect_t agent:coder start"));
    assert!(architect_policy.contains("allow architect_t agent:reviewer read"));
    assert!(architect_policy.contains("allow architect_t tool:fs.read execute"));

    let coder_policy = ok!(fs::read_to_string(root.join("agent/coder.d/policy")));
    assert!(coder_policy.contains("allow coder_t tool:fs.write execute"));
    assert!(coder_policy.contains("allow coder_t tool:shell.exec execute"));
    assert!(coder_policy.contains("allow coder_t tool:bash execute"));

    for index in ["by-cwd", "by-hash", "by-uuid"] {
        assert!(
            agent_session_root(root, "coder")
                .join("index")
                .join(index)
                .is_dir()
        );
    }
}

fn assert_object_hook_dirs(control_dir: &Path) {
    let hook_dir = control_dir.join(OBJECT_HOOK_DIR);
    assert!(hook_dir.is_dir());
    for phase in OBJECT_HOOK_PHASE_DIRS {
        assert!(hook_dir.join(phase).is_dir());
    }
}
#[test]
fn reference_tree_bootstrap_accepts_socket_symlink_without_changing_target() {
    let root = clean_test_dir("reference-tree-socket-symlink-mode");
    let outside = clean_test_dir("reference-tree-socket-symlink-mode-outside");
    assert!(fs::create_dir_all(&outside).is_ok());
    let outside_socket = outside.join("runtime.sock");
    let listener = ok!(UnixListener::bind(&outside_socket));
    set_file_mode(&outside_socket, 0o600);
    let target_owner = fs::symlink_metadata(&outside_socket)
        .map(|metadata| (metadata.uid(), metadata.gid()))
        .ok();
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    assert!(symlink(&outside_socket, root.join("agent").join("coder.sock")).is_ok());

    let bootstrapped = ensure_reference_tree(&root);

    assert!(bootstrapped.is_ok());
    assert!(
        root.join("agent")
            .join("coder.sock")
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
    let target_mode =
        fs::metadata(&outside_socket).map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(target_mode, Ok(0o600)));
    assert_eq!(
        fs::symlink_metadata(&outside_socket)
            .map(|metadata| (metadata.uid(), metadata.gid()))
            .ok(),
        target_owner
    );
    drop(listener);
    let _ignored = fs::remove_dir_all(outside);
}
use super::*;
