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
        assert_eq!(
            classify_abi_path(&format!("model/{model}")),
            "ctx.model.exec"
        );
    }
    assert_eq!(classify_abi_path("model/debug/echo"), "ctx.model.exec");
    assert_eq!(
        classify_abi_path("model/debug/echo.sock"),
        "ctx.model.socket"
    );
    assert_eq!(
        classify_abi_path("model/debug/echo.d/id"),
        "ctx.model.control"
    );
    assert_eq!(classify_abi_path("agent/coder"), "ctx.agent.exec");
    assert_eq!(classify_abi_path("agent/coder.sock"), "ctx.agent.socket");
    assert_eq!(
        classify_abi_path("agent/coder.d/policy"),
        "ctx.agent.control"
    );
    assert_eq!(classify_abi_path("tool/fs.read"), "ctx.tool.exec");
    assert_eq!(
        classify_abi_path("tool/fs.read.d/schema"),
        "ctx.tool.control"
    );
    assert_eq!(classify_abi_path("home/1000"), "ctx.home.dir");
    assert_eq!(
        classify_abi_path("home/1000/agent/coder/session/default"),
        "ctx.session.dir"
    );
    assert_eq!(
        classify_abi_path("home/1000/agent/coder/session/default/messages.jsonl"),
        "ctx.session.messages"
    );
    assert_eq!(
        classify_abi_path("home/1000/agent/coder/session/default/events.jsonl"),
        "ctx.session.events"
    );
    assert_eq!(
        classify_abi_path("home/1000/model/debug/echo.d/session/default"),
        "ctx.session.dir"
    );
    assert_eq!(
        classify_abi_path("shared/im-qq-dev/agent/bot/session/group-456/events.jsonl"),
        "ctx.session.events"
    );
    assert_eq!(
        classify_abi_path("shared/project-a/model/debug/echo.d/session/default/messages.jsonl"),
        "ctx.session.messages"
    );
    assert_eq!(classify_abi_path("shared/project-a"), "ctx.shared.dir");
    assert_eq!(
        classify_abi_path("shared/project-a/tool/project.test"),
        "ctx.shared.tool.exec"
    );
    assert_eq!(
        classify_abi_path("shared/project-a/tool/project.test.d/schema"),
        "ctx.shared.tool.control"
    );
    assert_eq!(
        classify_abi_path("shared/project-a/queue"),
        "ctx.shared.queue"
    );
    assert_eq!(
        classify_abi_path("shared/project-a/queue/pending"),
        "ctx.shared.queue"
    );
    assert_eq!(
        classify_abi_path("shared/project-a/result"),
        "ctx.shared.result"
    );
}

#[test]
fn abi_path_classifier_rejects_forbidden_root_and_bad_names() {
    assert_eq!(classify_abi_path("provider/openai"), "ctx.unknown");
    assert_eq!(classify_abi_path("mcp/github"), "ctx.unknown");
    assert_eq!(classify_abi_path("skill/local"), "ctx.unknown");
    assert_eq!(classify_abi_path("cluster/default"), "ctx.unknown");
    assert_eq!(
        classify_abi_path("model/debug/echo.sock.d/id"),
        "ctx.unknown"
    );
    assert_eq!(classify_abi_path("tool/-bad"), "ctx.unknown");
    assert_eq!(classify_abi_path("agent/coder/extra"), "ctx.unknown");
}

#[test]
fn reference_tree_bootstrap_materializes_documented_v1_shape() {
    let root = clean_test_dir("reference-tree");
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
    assert!(fs::create_dir_all(root.join("home").join("1000").join("tool")).is_ok());
    assert!(symlink(
        Path::new("/ctx/tool/fs.read"),
        root.join("home").join("1000").join("tool").join("fs.read")
    )
    .is_ok());

    let bootstrapped = ensure_v1_reference_tree(&root);
    let bootstrapped = ok!(bootstrapped);
    assert_eq!(bootstrapped.root(), root.as_path());

    let status = fs::read_to_string(root.join("status"));
    assert!(matches!(status, Ok(ref content) if content == "ready\n"));
    let status_mode = fs::metadata(root.join("status"))
        .map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(status_mode, Ok(0o644)));
    assert!(root.join("bin").join("ctx").is_file());
    assert!(!root.join("model").join("debug").join("echo").exists());
    let agent_socket_mode = fs::metadata(root.join("agent").join("coder.sock"))
        .map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(agent_socket_mode, Ok(0o777)));
    assert!(!root.join("mcp").exists());
    assert!(!root.join("skill").exists());
    assert!(!root.join("memory").exists());

    assert!(inspect_object_layout(&root, ObjectClass::Model, "debug/echo").is_ok());
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "reviewer").is_ok());
    for tool in ["fs.read", "fs.write", "shell.exec"] {
        assert!(inspect_object_layout(&root, ObjectClass::Tool, tool).is_ok());
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

    for (tool, required) in [
        ("fs.read", &["path"][..]),
        ("fs.write", &["path", "content"][..]),
        ("shell.exec", &["cmd"][..]),
    ] {
        let schema = fs::read_to_string(root.join("tool").join(format!("{tool}.d/schema")));
        let schema = ok!(schema);
        assert!(inspect_tool_schema_json(&schema).is_ok());
        let parsed = serde_json::from_str::<serde_json::Value>(&schema);
        let parsed = ok!(parsed);
        for field in required {
            assert!(parsed
                .pointer("/required")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(field))));
            assert!(parsed.pointer(&format!("/properties/{field}")).is_some());
        }
    }

    let private_session_root = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session");
    assert!(private_session_root.join("index").join("by-cwd").is_dir());
    assert!(!private_session_root.join("default").exists());

    assert!(root.join("home").join("1000").join("tool").is_dir());
    assert!(!root
        .join("home")
        .join("1000")
        .join("tool")
        .join("fs.read")
        .exists());
    let model_link = fs::read_link(root.join("home").join("1000").join("model").join("coder"));
    assert!(matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/debug/echo")));

    assert!(root.join("shared").is_dir());
    assert!(!root.join("shared").join("project-a").exists());

    assert_eq!(ensure_v1_reference_tree(&root), Ok(bootstrapped));

    let _ignored = fs::remove_dir_all(&root);
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

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn reference_tree_bootstrap_migrates_legacy_single_component_model_alias() {
    let root = clean_test_dir("reference-tree-legacy-model-alias");
    assert!(fs::create_dir_all(root.join("model")).is_ok());
    assert!(fs::create_dir_all(root.join("home").join("1000").join("model")).is_ok());
    assert!(symlink("gpt-5.4-mini", root.join("model").join("main")).is_ok());
    assert!(symlink(
        "/ctx/model/qwen",
        root.join("home").join("1000").join("model").join("coder")
    )
    .is_ok());
    write_text_file(
        &root
            .join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session")
            .join("default")
            .join("meta.json"),
        "{\"client\":\"ctx\",\"model\":\"main\",\"scope\":\"private\"}\n",
    );
    write_text_file(
        &root
            .join("shared")
            .join("project-a")
            .join("agent")
            .join("coder")
            .join("session")
            .join("design-review")
            .join("meta.json"),
        "{\"client\":\"ctx\",\"model\":\"qwen\",\"scope\":\"shared\"}\n",
    );

    assert!(ensure_v1_reference_tree(&root).is_ok());

    let agent_model = fs::read_to_string(root.join("agent").join("coder.d").join("model"));
    assert!(matches!(agent_model, Ok(ref content) if content == "debug/echo\n"));
    let agent_policy = fs::read_to_string(root.join("agent").join("coder.d").join("policy"));
    assert!(
        matches!(agent_policy, Ok(ref content) if content.contains("model:debug/echo use"))
    );
    let model_link = fs::read_link(root.join("home").join("1000").join("model").join("coder"));
    assert!(matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/debug/echo")));
    let private_meta = fs::read_to_string(
        root.join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session")
            .join("default")
            .join("meta.json"),
    );
    assert!(matches!(private_meta, Ok(ref content) if content.contains("\"model\":\"debug/echo\"")));
    let shared_meta = fs::read_to_string(
        root.join("shared")
            .join("project-a")
            .join("agent")
            .join("coder")
            .join("session")
            .join("design-review")
            .join("meta.json"),
    );
    assert!(matches!(shared_meta, Ok(ref content) if content.contains("\"model\":\"debug/echo\"")));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn reference_tree_bootstrap_preserves_valid_provider_model_alias() {
    let root = clean_test_dir("reference-tree-valid-model-alias");
    assert!(fs::create_dir_all(root.join("home").join("1000").join("model")).is_ok());
    assert!(symlink(
        "/ctx/model/openai/gpt-4o",
        root.join("home").join("1000").join("model").join("coder")
    )
    .is_ok());

    assert!(ensure_v1_reference_tree(&root).is_ok());

    let model_link = fs::read_link(root.join("home").join("1000").join("model").join("coder"));
    assert!(matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/openai/gpt-4o")));

    let _ignored = fs::remove_dir_all(&root);
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

    let _ignored = fs::remove_dir_all(&root);
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
fn reference_tree_standard_tools_emit_jsonl() {
    let root = reference_tree("reference-tree-tool-exec");

    let data = root.join("shared").join("project-a").join("data");
    let read_target = data.join("readme.txt");
    write_text_file(&read_target, "visible");
    let read_arg = format!(r#"{{"path":"{}"}}"#, read_target.display());
    let read = Command::new(root.join("tool").join("fs.read"))
        .arg(read_arg)
        .output();
    let read = ok!(read);
    assert!(read.status.success());
    let read_stdout = String::from_utf8(read.stdout);
    let read_stdout = ok!(read_stdout);
    assert!(read_stdout.contains(r#"{"type":"start","run":"r1","tool":"fs.read"}"#));
    assert!(read_stdout.contains(r#""text":"visible""#));
    assert!(inspect_event_stream_jsonl(&read_stdout).is_ok());

    let write_target = data.join("written.txt");
    let write_arg = format!(
        r#"{{"path":"{}","content":"stored"}}"#,
        write_target.display()
    );
    let write = Command::new(root.join("tool").join("fs.write"))
        .arg(write_arg)
        .output();
    let write = ok!(write);
    assert!(write.status.success());
    let written = fs::read_to_string(&write_target);
    assert!(matches!(written, Ok(ref content) if content == "stored"));
    let write_stdout = String::from_utf8(write.stdout);
    let write_stdout = ok!(write_stdout);
    assert!(write_stdout.contains(r#"{"type":"start","run":"r1","tool":"fs.write"}"#));
    assert!(inspect_event_stream_jsonl(&write_stdout).is_ok());

    let shell = Command::new(root.join("tool").join("shell.exec"))
        .arg(r#"{"cmd":"printf shell-ok"}"#)
        .output();
    let shell = ok!(shell);
    assert!(shell.status.success());
    let shell_stdout = String::from_utf8(shell.stdout);
    let shell_stdout = ok!(shell_stdout);
    assert!(shell_stdout.contains(r#"{"type":"start","run":"r1","tool":"shell.exec"}"#));
    assert!(shell_stdout.contains(r#""text":"shell-ok""#));
    assert!(inspect_event_stream_jsonl(&shell_stdout).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

