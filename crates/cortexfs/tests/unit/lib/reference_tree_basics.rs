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
    assert_file_text(&root.join("agent").join("base.d").join("parent"), "\n");
    assert_file_text(&root.join("agent").join("base.d").join("cwd"), "/workspace\n");
    assert_file_text(&root.join("agent").join("coder.d").join("cwd"), "/workspace\n");
    assert_file_text(
        &root.join("agent").join("coder.d").join("parent"),
        "agent:base\n",
    );
    assert_file_text(
        &root.join("agent").join("reviewer.d").join("parent"),
        "agent:base\n",
    );
    let base_policy = fs::read_to_string(root.join("agent").join("base.d").join("policy"));
    let base_policy = ok!(base_policy);
    assert!(base_policy.contains("allow base_t tool:tsh execute\n"));
    assert!(base_policy.contains("allow base_t agent:coder create\n"));
    assert!(base_policy.contains("allow base_t agent:reviewer start\n"));
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
    assert!(private_session_root.join("index").join("by-cwd").is_dir());
    assert!(!private_session_root.join("default").exists());
    assert!(agent_session_root(&root, "base")
        .join("index")
        .join("by-cwd")
        .is_dir());

    assert!(user_tool_dir.is_dir());
    assert!(!user_tool_dir.join("fs.read").exists());
    let model_link = fs::read_link(ctx_home(&root).join("model").join("coder"));
    assert!(matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/main")));

    assert!(root.join("shared").is_dir());
    assert!(!root.join("shared").join("project-a").exists());

    assert_eq!(ensure_v1_reference_tree(&root), Ok(bootstrapped));
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
    let agent_policy = fs::read_to_string(root.join("agent").join("coder.d").join("policy"));
    assert!(
        matches!(agent_policy, Ok(ref content) if content.contains("model:main use"))
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
