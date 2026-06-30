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
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "base").is_ok());
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "reviewer").is_ok());
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "executor").is_ok());
    assert_file_text(&root.join("agent").join("base.d").join("parent"), "\n");
    assert_file_text(&root.join("agent").join("base.d").join("cwd"), "/workspace\n");
    assert_file_text(&root.join("agent").join("coder.d").join("cwd"), "/workspace\n");
    assert_file_text(&root.join("agent").join("coder.d").join("model"), "main\n");
    assert_file_text(
        &root.join("agent").join("reviewer.d").join("model"),
        "helper\n",
    );
    assert_file_text(
        &root.join("agent").join("executor.d").join("model"),
        "openai/gpt-5.3-codex-spark\n",
    );
    assert_file_text(
        &root.join("agent").join("coder.d").join("parent"),
        "agent:base\n",
    );
    assert_file_text(
        &root.join("agent").join("reviewer.d").join("parent"),
        "agent:base\n",
    );
    assert_file_text(
        &root.join("agent").join("executor.d").join("parent"),
        "agent:base\n",
    );
    let base_policy = fs::read_to_string(root.join("agent").join("base.d").join("policy"));
    let base_policy = ok!(base_policy);
    assert!(base_policy.contains("allow base_t tool:tsh execute\n"));
    assert!(base_policy.contains("allow base_t tool:fs.read execute\n"));
    assert!(base_policy.contains("allow base_t network:default connect\n"));
    assert!(base_policy.contains("allow base_t agent:coder create\n"));
    assert!(base_policy.contains("allow base_t agent:reviewer start\n"));
    assert!(base_policy.contains("allow base_t agent:executor start\n"));
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
    for agent in ["base", "executor"] {
        for index in ["by-cwd", "by-hash", "by-uuid"] {
            assert!(agent_session_root(&root, agent)
                .join("index")
                .join(index)
                .is_dir());
        }
    }

    assert!(user_tool_dir.is_dir());
    assert!(!user_tool_dir.join("fs.read").exists());
    let model_link = fs::read_link(ctx_home(&root).join("model").join("coder"));
    assert!(matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/main")));

    assert!(root.join("shared").is_dir());
    assert!(!root.join("shared").join("project-a").exists());

    assert_eq!(ensure_v1_reference_tree(&root), Ok(bootstrapped));
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

fn assert_reference_bin_placeholders(root: &Path) {
    assert_file_text(
        &root.join("bin").join("ctx"),
        "#!/bin/sh\n# CortexFS reference-tree ctx placeholder.\nexec /usr/bin/ctx \"$@\"\n",
    );
    assert_file_text(
        &root.join("bin").join("ctxterm"),
        "#!/bin/sh\n# CortexFS reference-tree ctxterm placeholder.\nexec /usr/bin/ctxterm \"$@\"\n",
    );
    assert_file_text(
        &root.join("bin").join("tsh"),
        "#!/bin/sh\n# CortexFS reference-tree tsh placeholder.\nexec /usr/bin/tsh \"$@\"\n",
    );
}

#[test]
fn reference_tree_bootstrap_repairs_control_file_modes() {
    let root = clean_test_dir("reference-tree-control-mode");
    let status = root.join("tool").join("tsh.d").join("status");
    assert!(fs::create_dir_all(status.parent().unwrap_or(root.as_path())).is_ok());
    assert!(fs::write(&status, "idle\n").is_ok());
    assert!(fs::set_permissions(&status, fs::Permissions::from_mode(0o600)).is_ok());

    assert!(ensure_v1_reference_tree(&root).is_ok());

    let mode = fs::metadata(status).map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(mode, Ok(0o644)));
}

#[test]
fn reference_tree_bootstrap_replaces_tshrc_symlink_without_chmodding_target() {
    let root = clean_test_dir("reference-tree-tshrc-symlink");
    let victim = clean_test_dir("reference-tree-tshrc-victim");
    let victim_target = victim.join("target");
    assert!(fs::create_dir_all(&victim).is_ok());
    assert!(fs::write(&victim_target, "keep-private\n").is_ok());
    assert!(fs::set_permissions(&victim_target, fs::Permissions::from_mode(0o600)).is_ok());

    let tshrc = ctx_home(&root).join(".tshrc");
    assert!(fs::create_dir_all(ctx_home(&root)).is_ok());
    assert!(symlink(&victim_target, &tshrc).is_ok());

    let bootstrapped = ensure_v1_reference_tree(&root);
    assert!(bootstrapped.is_ok());

    let target_mode = fs::metadata(&victim_target)
        .map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(target_mode, Ok(0o600)));
    let tshrc_metadata = ok!(fs::symlink_metadata(&tshrc));
    assert!(tshrc_metadata.is_file());
    assert_eq!(tshrc_metadata.permissions().mode() & 0o777, 0o644);
    assert_file_text(&tshrc, "CTX_PATH=/ctx/tool:/ctx/home/1000/tool\n");
}

#[test]
fn reference_tree_bootstrap_rejects_symlinked_home_directory_without_writing_target() {
    let root = clean_test_dir("reference-tree-home-dir-symlink");
    let outside = clean_test_dir("reference-tree-home-dir-symlink-outside");
    assert!(fs::create_dir_all(root.join("home")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, root.join("home").join("1000")).is_ok());

    assert_eq!(
        ensure_v1_reference_tree(&root),
        Err(ReferenceTreeError::CannotCreate)
    );
    assert!(!outside.join(".tshrc").exists());
    assert!(!outside.join("agent").exists());
    assert!(!outside.join("tool").exists());
}

#[test]
fn reference_tree_bootstrap_rejects_symlinked_home_parent_without_writing_target() {
    let root = clean_test_dir("reference-tree-home-parent-symlink");
    let outside = clean_test_dir("reference-tree-home-parent-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, root.join("home")).is_ok());

    assert_eq!(
        ensure_v1_reference_tree(&root),
        Err(ReferenceTreeError::CannotCreate)
    );
    assert!(!outside.join("1000").exists());
    assert!(!outside.join("cortexfs-docs").exists());
}

#[test]
fn reference_tree_bootstrap_does_not_chown_descendants_through_symlink() {
    if !nix::unistd::Uid::effective().is_root() {
        return;
    }

    let root = clean_test_dir("reference-tree-chown-symlink-race");
    let victim = clean_test_dir("reference-tree-chown-victim");
    assert!(fs::create_dir_all(&victim).is_ok());
    let victim_target = victim.join("target");
    assert!(fs::write(&victim_target, "keep-root-owned\n").is_ok());
    assert!(nix::unistd::chown(
        &victim_target,
        Some(nix::unistd::Uid::from_raw(0)),
        Some(nix::unistd::Gid::from_raw(0)),
    )
    .is_ok());

    let attacker_link = ctx_home(&root).join("attacker-link");
    assert!(fs::create_dir_all(ctx_home(&root)).is_ok());
    assert!(symlink(&victim, &attacker_link).is_ok());

    let bootstrapped = ensure_v1_reference_tree(&root);
    assert!(bootstrapped.is_ok());

    let metadata = ok!(fs::symlink_metadata(&victim_target));
    assert_eq!(metadata.uid(), 0);
    assert_eq!(metadata.gid(), 0);
}

#[test]
fn reference_tree_model_exec_is_readonly_metadata() {
    let root = reference_tree("reference-tree-model-metadata");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    let metadata = projection.read_to_string("model/debug/echo");
    let metadata = ok!(metadata);
    assert!(metadata.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n")));
    assert!(metadata.contains("# cortexfs.object=model\n"));
    assert!(metadata.contains("# cortexfs.id=debug/echo\n"));
    assert!(metadata.contains("# cortexfs.name=debug/echo\n"));
    assert!(metadata.contains("# cortexfs.description=Built-in debug echo model\n"));
    assert!(metadata.contains("# cortexfs.type=debug\n"));
    assert!(metadata.contains("# cortexfs.created_at=\n"));
    assert!(metadata.contains("# cortexfs.owned_by=cortexfs\n"));
    assert!(metadata.contains("# cortexfs.context_length=0\n"));
    assert!(metadata.contains("# cortexfs.driver=debug\n"));
    assert!(metadata.contains("# cortexfs.driver.default=debug\n"));
    assert!(metadata.contains("# cortexfs.driver.exec=debug\n"));
    assert!(metadata.contains("# cortexfs.driver.socket=\n"));
    assert!(metadata.contains("# cortexfs.driver.agent=debug\n"));
    let permissions = projection
        .getattr("model/debug/echo")
        .map(|attr| attr.mode() & 0o777);
    assert!(matches!(permissions, Ok(0o555)));
    let driver_permissions = projection
        .getattr("model/debug/echo.d/driver")
        .map(|attr| attr.mode() & 0o777);
    assert!(matches!(driver_permissions, Ok(0o644)));
}

#[test]
fn reference_tree_bootstrap_migrates_legacy_single_component_model_alias() {
    let root = clean_test_dir("reference-tree-legacy-model-alias");
    let user_model = ctx_home(&root).join("model");
    let shared_meta_path = fixture_path(
        &root,
        &[
            "shared",
            "project-a",
            "agent",
            "coder",
            "session",
            "design-review",
            "meta.json",
        ],
    );
    assert!(fs::create_dir_all(root.join("model")).is_ok());
    assert!(fs::create_dir_all(&user_model).is_ok());
    assert!(symlink("gpt-5.4-mini", root.join("model").join("main")).is_ok());
    assert!(symlink("/ctx/model/qwen", user_model.join("coder")).is_ok());
    write_text_file(
        &agent_session_root(&root, "coder")
            .join("default")
            .join("meta.json"),
        "{\"client\":\"ctx\",\"model\":\"main\",\"scope\":\"private\"}\n",
    );
    write_text_file(
        &shared_meta_path,
        "{\"client\":\"ctx\",\"model\":\"qwen\",\"scope\":\"shared\"}\n",
    );

    assert!(ensure_v1_reference_tree(&root).is_ok());

    assert_file_text(&root.join("agent").join("coder.d").join("model"), "main\n");
    assert_file_text(
        &root.join("agent").join("coder.d").join("system.md"),
        "You are CortexFS agent `coder`.\n",
    );
    let prompt_template = fs::read_to_string(
        root.join("agent")
            .join("coder.d")
            .join("prompt.template.md"),
    );
    assert!(
        matches!(prompt_template, Ok(ref content) if content.contains("{{agent_instructions}}"))
    );
    let agent_script = fs::read_to_string(root.join("agent").join("coder"));
    assert!(
        matches!(agent_script, Ok(ref content) if content.contains("CTX_AGENT_PROMPT_TEMPLATE"))
    );
    assert!(
        matches!(agent_script, Ok(ref content) if content.contains("/usr/bin/cat")
            && content.contains("/usr/bin/tr")
            && content.contains("/usr/bin/readlink"))
    );
    let agent_policy = fs::read_to_string(root.join("agent").join("coder.d").join("policy"));
    assert!(
        matches!(agent_policy, Ok(ref content) if content.contains("model:main use"))
    );
    assert!(
        matches!(agent_policy, Ok(ref content) if content.contains("network:default connect"))
    );
    let model_link = fs::read_link(user_model.join("coder"));
    assert!(matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/main")));
    let private_meta =
        fs::read_to_string(agent_session_root(&root, "coder").join("default").join("meta.json"));
    assert!(matches!(private_meta, Ok(ref content) if content.contains("\"model\":\"main\"")));
    let shared_meta = fs::read_to_string(shared_meta_path);
    assert!(matches!(shared_meta, Ok(ref content) if content.contains("\"model\":\"debug/echo\"")));
}

#[test]
fn reference_tree_bootstrap_ignores_symlink_session_meta_during_migration() {
    let root = clean_test_dir("reference-tree-session-meta-symlink");
    let outside = clean_test_dir("reference-tree-session-meta-symlink-outside");
    let session = agent_session_root(&root, "coder").join("default");
    assert!(fs::create_dir_all(&session).is_ok());
    write_text_file(&outside.join("meta.json"), "{\"model\":\"legacy\"}\n");
    assert!(symlink(outside.join("meta.json"), session.join("meta.json")).is_ok());

    assert!(ensure_v1_reference_tree(&root).is_ok());

    assert!(session
        .join("meta.json")
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink()));
    assert_file_text(&outside.join("meta.json"), "{\"model\":\"legacy\"}\n");
}

#[test]
fn reference_tree_bootstrap_ignores_symlink_session_dir_during_migration() {
    let root = clean_test_dir("reference-tree-session-dir-symlink");
    let outside = clean_test_dir("reference-tree-session-dir-symlink-outside");
    let agent = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("attacker");
    assert!(fs::create_dir_all(&agent).is_ok());
    write_text_file(
        &outside.join("default").join("meta.json"),
        "{\"model\":\"legacy\"}\n",
    );
    assert!(symlink(&outside, agent.join("session")).is_ok());

    assert!(ensure_v1_reference_tree(&root).is_ok());

    assert_file_text(&outside.join("default").join("meta.json"), "{\"model\":\"legacy\"}\n");
}

#[test]
fn reference_tree_bootstrap_preserves_valid_provider_model_alias() {
    let root = clean_test_dir("reference-tree-valid-model-alias");
    let user_model = ctx_home(&root).join("model");
    assert!(fs::create_dir_all(&user_model).is_ok());
    assert!(symlink("/ctx/model/openai/gpt-4o", user_model.join("coder")).is_ok());

    assert!(ensure_v1_reference_tree(&root).is_ok());

    let model_link = fs::read_link(user_model.join("coder"));
    assert!(matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/openai/gpt-4o")));
}

#[test]
fn reference_tree_bootstrap_rejects_symlink_model_alias_parent_without_writing_target() {
    let root = clean_test_dir("reference-tree-model-alias-parent-symlink");
    let outside = clean_test_dir("reference-tree-model-alias-parent-symlink-outside");
    let user = root.join("home").join("1000");
    assert!(fs::create_dir_all(&user).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, user.join("model")).is_ok());

    assert!(ensure_v1_reference_tree(&root).is_err());
    assert!(!outside.join("coder").exists());
}

#[test]
fn model_exec_metadata_exposes_driver_route_table() {
    let root = clean_test_dir("model-driver-metadata");
    let control = root.join("model").join("openai").join("gpt-4o.d");

    write_text_file(&control.join("id"), "openai/gpt-4o\n");
    write_text_file(
        &control.join("driver"),
        "default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n",
    );
    write_text_file(&control.join("cap"), "chat\nstream\ntool_call_syntax\n");
    write_text_file(&control.join("session"), "socket\n");
    write_text_file(&control.join("status"), "idle\n");

    let metadata = model_exec_metadata("openai/gpt-4o", &control);
    let metadata = ok!(metadata);
    assert!(metadata.contains("# cortexfs.driver=openai-chat\n"));
    assert!(metadata.contains("# cortexfs.driver.default=openai-chat\n"));
    assert!(metadata.contains("# cortexfs.driver.exec=openai-chat\n"));
    assert!(metadata.contains("# cortexfs.driver.socket=\n"));
    assert!(metadata.contains("# cortexfs.driver.agent=openai-responses,openai-chat\n"));
}

#[test]
fn model_exec_metadata_refuses_symlink_control_files() {
    let root = clean_test_dir("model-driver-metadata-symlink");
    let control = root.join("model").join("openai").join("gpt-4o.d");
    let outside = root.join("outside-driver");

    write_text_file(&control.join("id"), "openai/gpt-4o\n");
    write_text_file(&outside, "default=openai-chat\n");
    assert!(symlink(&outside, control.join("driver")).is_ok());
    write_text_file(&control.join("cap"), "chat\n");
    write_text_file(&control.join("session"), "socket\n");
    write_text_file(&control.join("status"), "idle\n");

    assert_eq!(
        model_exec_metadata("openai/gpt-4o", &control),
        Err(FuseV1Error::InvalidContent)
    );
}

#[test]
fn model_exec_metadata_refuses_symlink_control_directory() {
    let root = clean_test_dir("model-driver-metadata-symlink-dir");
    let outside = clean_test_dir("model-driver-metadata-symlink-dir-outside");
    let control = root.join("model").join("openai").join("gpt-4o.d");
    let outside_control = outside.join("gpt-4o.d");

    write_text_file(&outside_control.join("id"), "openai/gpt-4o\n");
    write_text_file(&outside_control.join("driver"), "default=openai-chat\n");
    write_text_file(&outside_control.join("cap"), "chat\n");
    write_text_file(&outside_control.join("session"), "socket\n");
    write_text_file(&outside_control.join("status"), "idle\n");
    assert!(fs::create_dir_all(root.join("model").join("openai")).is_ok());
    assert!(symlink(&outside_control, &control).is_ok());

    assert_eq!(
        model_exec_metadata("openai/gpt-4o", &control),
        Err(FuseV1Error::Io)
    );
}

#[test]
fn model_exec_metadata_refuses_oversized_control_files() {
    let root = clean_test_dir("model-driver-metadata-oversized");
    let control = root.join("model").join("openai").join("gpt-4o.d");

    write_text_file(&control.join("id"), "openai/gpt-4o\n");
    write_text_file(&control.join("driver"), &"x".repeat((64 * 1024) + 1));
    write_text_file(&control.join("cap"), "chat\n");
    write_text_file(&control.join("session"), "socket\n");
    write_text_file(&control.join("status"), "idle\n");

    assert_eq!(
        model_exec_metadata("openai/gpt-4o", &control),
        Err(FuseV1Error::InvalidContent)
    );
}

#[test]
fn echo_model_runner_emits_one_shot_jsonl() {
    let mut stdout = Vec::new();
    let result = run_echo_model(["fix tests"], &mut stdout);
    assert!(result.is_ok());
    let stdout = String::from_utf8(stdout);
    let stdout = ok!(stdout);
    assert!(stdout.contains(r#"{"type":"start","run":"r1","model":"debug/echo"}"#));
    assert!(stdout.contains(r#"{"type":"delta","run":"r1","text":"fix tests"}"#));
    assert!(stdout.contains(r#"{"type":"done","run":"r1","status":"ok"}"#));
    assert!(inspect_event_stream_jsonl(&stdout).is_ok());
}

#[test]
fn echo_model_stdin_reader_accepts_input_at_limit() {
    let input = "x".repeat(MAX_ECHO_MODEL_STDIN_BYTES);

    let read = read_echo_model_stdin_limited(
        std::io::Cursor::new(input.as_bytes()),
        MAX_ECHO_MODEL_STDIN_BYTES,
    );

    assert_eq!(read.unwrap_or_default().len(), MAX_ECHO_MODEL_STDIN_BYTES);
}

#[test]
fn echo_model_stdin_reader_rejects_input_over_limit() {
    let input = "x".repeat(MAX_ECHO_MODEL_STDIN_BYTES + 1);

    let read = read_echo_model_stdin_limited(
        std::io::Cursor::new(input.as_bytes()),
        MAX_ECHO_MODEL_STDIN_BYTES,
    );

    assert!(matches!(read, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
fn reference_tree_bootstrap_installs_tsh_tools() {
    let root = reference_tree("reference-tree-tool-exec");

    assert!(root.join("tool").join("tsh").is_file());
    assert!(root.join("tool").join("tsh.d").is_dir());
    assert!(root.join("tool").join("tsh.d").join("config").is_file());
    assert!(root.join("tool").join("tsh.config").is_file());
    assert!(root.join("tool").join("tsh.config.d").is_dir());
    for tool in ["fs.read", "fs.write", "shell.exec", "bash", "tmux", "zellij"] {
        assert!(!root.join("tool").join(tool).exists());
        assert!(!root.join("tool").join(format!("{tool}.d")).exists());
    }
}

#[test]
fn reference_tree_bootstrap_does_not_remove_symlinked_deprecated_tool_control_dir() {
    let root = clean_test_dir("ref-deprecated-tool-control-symlink");
    let outside = clean_test_dir("ref-deprecated-tool-control-symlink-outside");
    let tool_dir = root.join("tool");
    let control_link = tool_dir.join("agent.create.d");
    assert!(fs::create_dir_all(&tool_dir).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(
        &tool_dir.join("agent.create"),
        "#!/bin/sh\n# CortexFS generated object wrapper.\nexec '/bin/false' \"$0\" \"$@\"\n",
    );
    write_text_file(
        &outside.join("description"),
        "CortexFS reference-tree tool\n",
    );
    assert!(symlink(&outside, &control_link).is_ok());

    assert!(ensure_v1_reference_tree(&root).is_ok());

    assert!(control_link
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink()));
    assert_file_text(
        &tool_dir.join("agent.create"),
        "#!/bin/sh\n# CortexFS generated object wrapper.\nexec '/bin/false' \"$0\" \"$@\"\n",
    );
    assert_file_text(
        &outside.join("description"),
        "CortexFS reference-tree tool\n",
    );
}

#[test]
fn reference_tree_bootstrap_removes_exact_deprecated_placeholder_tool() {
    let root = clean_test_dir("reference-tree-deprecated-tool-exact");
    let tool_dir = root.join("tool");
    let control_dir = tool_dir.join("agent.create.d");
    assert!(fs::create_dir_all(&control_dir).is_ok());
    write_text_file(
        &tool_dir.join("agent.create"),
        "#!/bin/sh\n# CortexFS generated object wrapper.\nexec '/bin/false' \"$0\" \"$@\"\n",
    );
    for file in TOOL_CONTROL_FILES {
        write_text_file(
            &control_dir.join(file),
            if *file == "description" {
                "CortexFS reference-tree tool\n"
            } else {
                "\n"
            },
        );
    }

    assert!(ensure_v1_reference_tree(&root).is_ok());

    assert!(!tool_dir.join("agent.create").exists());
    assert!(!control_dir.exists());
}

#[test]
fn reference_tree_bootstrap_preserves_deprecated_placeholder_tool_with_unknown_control_file() {
    let root = clean_test_dir("reference-tree-deprecated-tool-extra-control");
    let tool_dir = root.join("tool");
    let control_dir = tool_dir.join("agent.create.d");
    assert!(fs::create_dir_all(&control_dir).is_ok());
    write_text_file(
        &tool_dir.join("agent.create"),
        "#!/bin/sh\n# CortexFS generated object wrapper.\nexec '/bin/false' \"$0\" \"$@\"\n",
    );
    for file in TOOL_CONTROL_FILES {
        write_text_file(
            &control_dir.join(file),
            if *file == "description" {
                "CortexFS reference-tree tool\n"
            } else {
                "\n"
            },
        );
    }
    write_text_file(&control_dir.join("user-note"), "keep me\n");

    assert!(ensure_v1_reference_tree(&root).is_ok());

    assert_file_text(
        &tool_dir.join("agent.create"),
        "#!/bin/sh\n# CortexFS generated object wrapper.\nexec '/bin/false' \"$0\" \"$@\"\n",
    );
    assert_file_text(&control_dir.join("user-note"), "keep me\n");
}

#[test]
fn root_bootstrap_chowns_reference_home_symlinks_without_following_targets() {
    if fs::metadata("/proc/self").map_or(1, |metadata| metadata.uid()) != 0 {
        return;
    }

    let root = clean_test_dir("reference-tree-home-symlink-ownership");
    let target_dir = clean_test_dir("reference-tree-home-symlink-target");
    let target = target_dir.join("root-owned-target");
    assert!(fs::create_dir_all(&target_dir).is_ok());
    assert!(fs::write(&target, "keep root owner\n").is_ok());
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let link = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session")
        .join("pwn");
    assert!(symlink(&target, &link).is_ok());

    let target_before = ok!(fs::symlink_metadata(&target));
    assert_eq!(target_before.uid(), 0);
    assert_eq!(target_before.gid(), 0);

    assert!(ensure_v1_reference_tree(&root).is_ok());

    let link_metadata = ok!(fs::symlink_metadata(&link));
    assert!(link_metadata.file_type().is_symlink());
    assert_eq!(link_metadata.uid(), 1000);
    assert_eq!(link_metadata.gid(), 1000);

    let target_after = ok!(fs::symlink_metadata(&target));
    assert_eq!(target_after.uid(), 0);
    assert_eq!(target_after.gid(), 0);
}

#[test]
fn root_bootstrap_assigns_reference_home_to_agent_identity() {
    if fs::metadata("/proc/self").map_or(1, |metadata| metadata.uid()) != 0 {
        return;
    }

    let root = clean_test_dir("reference-tree-home-ownership");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    for path in [
        root.join("home").join("1000"),
        root.join("home").join("1000").join("agent").join("coder"),
        root.join("home").join("1000").join("agent").join("coder").join("session"),
        root.join("home").join("1000").join("agent").join("coder").join("session").join("index"),
    ] {
        let metadata = ok!(fs::symlink_metadata(path));
        assert_eq!(metadata.uid(), 1000);
        assert_eq!(metadata.gid(), 1000);
    }
}
