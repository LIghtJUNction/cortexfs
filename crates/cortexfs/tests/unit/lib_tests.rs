use super::{
    authorize_child_agent, authorize_session_access, authorize_shared_access,
    authorize_tool_execution, claim_next_shared_queue_job, classify_abi_path,
    derive_agent_runtime_view, ensure_durable_session_layout, ensure_v1_reference_tree,
    finish_shared_queue_job, handle_socket_request_frame, inspect_agent_control,
    inspect_context_jsonl, inspect_context_pack_json, inspect_event_stream_jsonl,
    inspect_message_stream_jsonl, inspect_model_capabilities, inspect_object_layout,
    inspect_session_control, inspect_session_index, inspect_session_layout,
    inspect_shared_queue_layout, inspect_tool_schema_json, install_executable_object_wrapper,
    is_object_name, is_root_entry, model_exec_metadata, owned_child_cancellation_events,
    parse_model_driver_routes, parse_socket_request_frame, peer_credentials, rebuild_context_pack,
    record_assistant_response_to_session, record_child_handoff_to_parent_context,
    record_child_result_to_parent_context, record_indexed_socket_send_to_session,
    record_owned_child_cancellation, record_socket_request_to_session,
    record_tool_execution_denial_to_session, record_tool_execution_result_to_session,
    recover_shared_queue_job, resolve_api_key_with, run_echo_model,
    serve_agent_executable_socket_stream_once, serve_unix_socket_listener_once,
    serve_unix_socket_stream_once, session_index_key_for_cwd, socket_runtime_error_response,
    update_session_index, validate_context_pack_source,
    AgentControlIssue, AgentControlKind, AgentExecutableSocketRuntime, AgentRuntimeViewError,
    AgentUnixIdentity, ApiKeyResolutionError, ChildAgentAuthority, ChildAgentControls,
    ChildAgentDenial, ChildAgentRequest, ChildContextRecordError, ChildContextStatus,
    ChildLifecycle, ContextJsonlIssue, ContextJsonlKind, ContextPackBuildError,
    ContextPackIssue, ContextPackSourceError, DurableSessionLayoutError, EventStreamIssue,
    FuseV1Error, FuseV1FileType, FuseV1Projection, IndexedSocketSessionRecordError,
    MessageStreamIssue, ModelCapabilityIssue, ModelDriverRouteError, ModelDriverUseCase,
    MountEntry, MountError, MountMode, MountOption, MountTable, ObjectBootstrapError, ObjectClass,
    ObjectLayoutIssue, OwnedChildCancellationError, PeerCredentials, PolicyError,
    PolicyObjectClass, PolicyPermission, PolicyRule, PolicyV0, ReferenceTreeError, SessionAccess,
    SessionAccessAuthority, SessionAccessDenial, SessionControlIssue, SessionControlKind,
    SessionIndexIssue, SessionIndexKind, SessionIndexUpdateError, SessionLayoutIssue,
    SharedAccess, SharedAccessAuthority, SharedAccessDenial, SharedQueueLayoutIssue,
    SharedQueueOutcome, SharedQueueRecoverError, SocketPeerPolicy, SocketRequest, SocketRequestError,
    SocketRuntimeError,
    SocketSessionRecordError, SocketSessionScope, ToolExecutionAuthority, ToolExecutionDenial,
    ToolExecutionPrincipal, ToolHit, ToolPath, ToolPathError, ToolSchemaIssue, AGENT_CONTROL_FILES,
    CORTEXFS_OBJECT_RUNNER, CTX_ROOT, EXEC_OBJECTS, FUSE_V1_ROOT_INODE,
    MAX_FUSE_V1_SMALL_WRITE_BYTES, MAX_OBJECT_NAME_LEN, MAX_SOCKET_FRAME_BYTES,
    MODEL_CONTROL_FILES, SESSION_REQUIRED_FILES, SHARED_QUEUE_REQUIRED_DIRS, TOOL_CONTROL_FILES,
};
use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{symlink, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let root = unique_test_dir("reference-tree");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
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
    assert!(bootstrapped.is_ok());
    let Ok(bootstrapped) = bootstrapped else {
        return;
    };
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
        assert!(schema.is_ok());
        let Ok(schema) = schema else { return };
        assert!(inspect_tool_schema_json(&schema).is_ok());
        let parsed = serde_json::from_str::<serde_json::Value>(&schema);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
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
    let root = unique_test_dir("reference-tree-model-metadata");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    let metadata = projection.read_to_string("model/debug/echo");
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
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
    let root = unique_test_dir("reference-tree-legacy-model-alias");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
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
    let root = unique_test_dir("reference-tree-valid-model-alias");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
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
    let root = unique_test_dir("model-driver-metadata");
    let control = root.join("model").join("openai").join("gpt-4o.d");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_text_file(&control.join("id"), "openai/gpt-4o\n");
    write_text_file(
        &control.join("driver"),
        "default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n",
    );
    write_text_file(&control.join("cap"), "chat\nstream\ntool_call_syntax\n");
    write_text_file(&control.join("session"), "socket\n");
    write_text_file(&control.join("status"), "idle\n");

    let metadata = model_exec_metadata("openai/gpt-4o", &control);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
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
    assert!(stdout.is_ok());
    let Ok(stdout) = stdout else { return };
    assert!(stdout.contains(r#"{"type":"start","run":"r1","model":"debug/echo"}"#));
    assert!(stdout.contains(r#"{"type":"delta","run":"r1","text":"fix tests"}"#));
    assert!(stdout.contains(r#"{"type":"done","run":"r1","status":"ok"}"#));
    assert!(inspect_event_stream_jsonl(&stdout).is_ok());
}

#[test]
fn reference_tree_standard_tools_emit_jsonl() {
    let root = unique_test_dir("reference-tree-tool-exec");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let data = root.join("shared").join("project-a").join("data");
    let read_target = data.join("readme.txt");
    write_text_file(&read_target, "visible");
    let read_arg = format!(r#"{{"path":"{}"}}"#, read_target.display());
    let read = Command::new(root.join("tool").join("fs.read"))
        .arg(read_arg)
        .output();
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    assert!(read.status.success());
    let read_stdout = String::from_utf8(read.stdout);
    assert!(read_stdout.is_ok());
    let Ok(read_stdout) = read_stdout else {
        return;
    };
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
    assert!(write.is_ok());
    let Ok(write) = write else { return };
    assert!(write.status.success());
    let written = fs::read_to_string(&write_target);
    assert!(matches!(written, Ok(ref content) if content == "stored"));
    let write_stdout = String::from_utf8(write.stdout);
    assert!(write_stdout.is_ok());
    let Ok(write_stdout) = write_stdout else {
        return;
    };
    assert!(write_stdout.contains(r#"{"type":"start","run":"r1","tool":"fs.write"}"#));
    assert!(inspect_event_stream_jsonl(&write_stdout).is_ok());

    let shell = Command::new(root.join("tool").join("shell.exec"))
        .arg(r#"{"cmd":"printf shell-ok"}"#)
        .output();
    assert!(shell.is_ok());
    let Ok(shell) = shell else { return };
    assert!(shell.status.success());
    let shell_stdout = String::from_utf8(shell.stdout);
    assert!(shell_stdout.is_ok());
    let Ok(shell_stdout) = shell_stdout else {
        return;
    };
    assert!(shell_stdout.contains(r#"{"type":"start","run":"r1","tool":"shell.exec"}"#));
    assert!(shell_stdout.contains(r#""text":"shell-ok""#));
    assert!(inspect_event_stream_jsonl(&shell_stdout).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "single projection smoke test keeps related FUSE ABI assertions together"
)]
fn fuse_v1_projection_exposes_reference_tree_ops() {
    let root = unique_test_dir("fuse-v1-projection");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    write_fixture_file(&root.join("model").join("qwen"), 0o755);
    assert!(fs::create_dir_all(root.join("model").join("qwen.d")).is_ok());
    assert!(symlink("qwen", root.join("model").join("main")).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    let root_node = projection.root_node();
    assert!(root_node.is_ok());
    let Ok(root_node) = root_node else { return };
    assert_eq!(root_node.inode(), FUSE_V1_ROOT_INODE);
    assert_eq!(root_node.abi_path(), "");
    assert_eq!(root_node.attr().file_type(), FuseV1FileType::Directory);

    let root_attr = projection.getattr_node(&root_node);
    assert!(matches!(
        root_attr,
        Ok(ref attr)
            if attr.abi_path().is_empty()
                && attr.file_type() == FuseV1FileType::Directory
    ));

    let entries = projection.readdir_node(&root_node);
    assert!(entries.is_ok());
    let Ok(entries) = entries else { return };
    let names = entries
        .iter()
        .map(super::FuseV1DirEntry::name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["agent", "bin", "home", "model", "shared", "status", "tool"]
    );

    let model_node = projection.lookup(&root_node, "model");
    assert!(matches!(
        model_node,
        Ok(ref node)
            if node.abi_path() == "model"
                && node.attr().file_type() == FuseV1FileType::Directory
    ));
    let Ok(model_node) = model_node else { return };
    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let Ok(model_entries) = model_entries else {
        return;
    };
    let model_names = model_entries
        .iter()
        .map(super::FuseV1DirEntry::name)
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["debug", "helper", "main"]);
    let main_node = projection.lookup(&model_node, "main");
    assert!(matches!(
        main_node,
        Ok(ref node)
            if node.abi_path() == "model/main"
                && node.attr().file_type() == FuseV1FileType::Symlink
    ));
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/debug/echo"))
    );
    assert_eq!(
        projection.readlink("model/helper"),
        Ok(PathBuf::from("/ctx/model/debug/echo"))
    );
    let debug_node = projection.lookup(&model_node, "debug");
    assert!(matches!(
        debug_node,
        Ok(ref node)
            if node.abi_path() == "model/debug"
                && node.inode() != FUSE_V1_ROOT_INODE
                && node.attr().file_type() == FuseV1FileType::Directory
    ));
    let Ok(debug_node) = debug_node else { return };
    let echo_node = projection.lookup(&debug_node, "echo");
    assert!(matches!(
        echo_node,
        Ok(ref node)
            if node.abi_path() == "model/debug/echo"
                && node.inode() != FUSE_V1_ROOT_INODE
                && node.attr().file_type() == FuseV1FileType::Regular
    ));
    let echo_again = projection.node_for_path("model/debug/echo");
    assert!(
        matches!((echo_node, echo_again), (Ok(ref left), Ok(ref right)) if left.inode() == right.inode())
    );
    let echo_metadata = projection.read_to_string("model/debug/echo");
    assert!(matches!(
        echo_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.name=debug/echo\n")
    ));
    assert_eq!(
        projection.read_at("model/debug/echo", 0, 32),
        Ok(echo_metadata
            .unwrap_or_default()
            .bytes()
            .take(32)
            .collect::<Vec<_>>())
    );
    let echo_attr = projection.getattr("model/debug/echo");
    assert!(matches!(
        echo_attr,
        Ok(ref attr) if attr.mode() & 0o777 == 0o555
    ));
    let tool_metadata = projection.read_to_string("tool/fs.read");
    assert!(matches!(
        tool_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.object=tool\n")
                && content.contains("# cortexfs.name=fs.read\n")
                && !content.contains("#!/bin/sh")
    ));
    let tool_attr = projection.getattr("tool/fs.read");
    assert!(matches!(
        tool_attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Regular && attr.mode() & 0o777 == 0o555
    ));
    assert_eq!(
        projection.lookup(&root_node, "../escape"),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        projection.lookup(&root_node, "missing"),
        Err(FuseV1Error::NotFound)
    );

    assert_eq!(
        projection.getattr("model/debug/echo.sock"),
        Err(FuseV1Error::NotFound)
    );
    let socket_attr = projection.getattr("agent/coder.sock");
    assert!(matches!(
        socket_attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Socket && attr.mode() & 0o777 == 0o777
    ));
    assert_eq!(
        projection.getattr("home/1000/tool/fs.read"),
        Err(FuseV1Error::NotFound)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn fuse_v1_projection_reads_and_writes_control_files() {
    let root = unique_test_dir("fuse-v1-projection-control-files");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.read_to_string("status"),
        Ok("ready\n".to_owned())
    );
    assert_eq!(projection.read_at("status", 1, 3), Ok(b"ead".to_vec()));
    assert_eq!(projection.read_at("status", 128, 8), Ok(Vec::new()));
    assert!(projection
        .write_control_file("agent/coder.d/cwd", "/work/project\n")
        .is_ok());
    assert_eq!(
        projection.read_to_string("agent/coder.d/cwd"),
        Ok("/work/project\n".to_owned())
    );

    assert_eq!(
        projection.write_control_file("status", "busy\n"),
        Err(FuseV1Error::NotControlFile)
    );
    assert!(projection
        .write_control_file_at("agent/coder.d/status", 0, b"busy\n")
        .is_ok());
    assert_eq!(
        projection.read_to_string("agent/coder.d/status"),
        Ok("busy\n".to_owned())
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/status", 1, b"idle\n"),
        Err(FuseV1Error::InvalidOffset)
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/status", 0, &[0xff]),
        Err(FuseV1Error::InvalidContent)
    );
    assert_eq!(
        projection.write_control_file("../escape", "no\n"),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        projection.write_control_file(
            "agent/coder.d/cwd",
            &"x".repeat(MAX_FUSE_V1_SMALL_WRITE_BYTES + 1)
        ),
        Err(FuseV1Error::TooLarge)
    );
    assert_eq!(FuseV1Error::TooLarge.errno(), "EMSGSIZE");
    assert_eq!(FuseV1Error::InvalidOffset.errno(), "EINVAL");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn fuse_v1_projection_projects_configured_provider_models() {
    let root = unique_test_dir("fuse-v1-provider-model");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("api.lmm.best.json"),
        r#"{
  "base_url": "https://api.lmm.best:9000/",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
"#,
    );
    let projection = FuseV1Projection::new(&root).with_provider_config_dir(&providers);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["api.lmm.best", "debug", "helper", "main"]);

    let provider_entries = projection.readdir("model/api.lmm.best");
    assert!(provider_entries.is_ok());
    let provider_names = provider_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(provider_names, ["gpt-5.4-mini", "gpt-5.4-mini.d"]);

    let metadata = projection.read_to_string("model/api.lmm.best/gpt-5.4-mini");
    assert!(matches!(
        metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.name=api.lmm.best/gpt-5.4-mini\n")
                && content.contains("# cortexfs.driver.default=openai-chat\n")
                && content.contains("# cortexfs.driver.agent=openai-responses,openai-chat\n")
    ));
    assert_eq!(
        projection.read_to_string("model/api.lmm.best/gpt-5.4-mini.d/driver"),
        Ok("default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n".to_owned())
    );
    assert_eq!(
        projection.read_to_string("model/api.lmm.best/gpt-5.4-mini.d/default"),
        Ok("base_url=https://api.lmm.best:9000/\n".to_owned())
    );
    let attr = projection.getattr("model/api.lmm.best/gpt-5.4-mini");
    assert!(matches!(attr, Ok(ref attr) if attr.mode() & 0o777 == 0o555));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn fuse_v1_projection_skips_disabled_provider_models() {
    let root = unique_test_dir("fuse-v1-disabled-provider-model");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("api.lmm.best.json"),
        r#"{
  "base_url": "https://api.lmm.best:9000/",
  "default_model": "gpt-5.4-mini",
  "enabled": false,
  "formats": ["openai.chat"]
}
"#,
    );
    let projection = FuseV1Projection::new(&root).with_provider_config_dir(&providers);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["debug", "helper", "main"]);
    assert_eq!(
        projection.getattr("model/api.lmm.best"),
        Err(FuseV1Error::NotFound)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn reference_tree_bootstrap_rejects_conflicting_symlink_and_socket_paths() {
    let root = unique_test_dir("reference-tree-conflict");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_text_file(&root.join("home").join("1000").join("model").join("coder"), "not link\n");
    assert_eq!(
        ensure_v1_reference_tree(&root),
        Err(ReferenceTreeError::CannotLink)
    );

    assert!(fs::remove_dir_all(&root).is_ok());
    write_text_file(&root.join("agent").join("coder.sock"), "not socket\n");
    assert_eq!(
        ensure_v1_reference_tree(&root),
        Err(ReferenceTreeError::CannotSocket)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn reference_tree_bootstrap_replaces_stale_socket_symlink() {
    let root = unique_test_dir("reference-tree-stale-socket-symlink");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    assert!(symlink(
        root.join("missing-runtime.sock"),
        root.join("agent").join("coder.sock")
    )
    .is_ok());

    assert!(ensure_v1_reference_tree(&root).is_ok());
    let metadata = fs::symlink_metadata(root.join("agent").join("coder.sock"));
    assert!(matches!(metadata, Ok(ref metadata) if metadata.file_type().is_socket()));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn object_layout_accepts_model_agent_and_tool_triples() {
    let root = unique_test_dir("object-layout-ok");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Model, "debug/echo", "socket");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "");
    create_complete_object_layout(&root, ObjectClass::Tool, "fs.read", "");
    let _model_socket = bind_socket(&root.join("model").join("debug").join("echo.sock"));
    let _agent_socket = bind_socket(&root.join("agent").join("coder.sock"));

    let model = inspect_object_layout(&root, ObjectClass::Model, "debug/echo");
    let agent = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    let tool = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
    assert!(model.is_ok());
    assert!(agent.is_ok());
    assert!(tool.is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn executable_object_bootstrap_installs_model_and_tool_wrappers() {
    let root = unique_test_dir("object-bootstrap");
    let target = root.join("runtime").join("echo-jsonl");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_fixture_file(&target, 0o755);

    let model = install_executable_object_wrapper(
        &root,
        ObjectClass::Model,
        "debug/echo",
        &target.display().to_string(),
        &[
            ("cap", "chat\nstream\ntool_call_syntax"),
            ("session", "none"),
            ("id", "debug/echo"),
        ],
    );
    assert!(model.is_ok());
    let Ok(model) = model else { return };
    let tool = install_executable_object_wrapper(
        &root,
        ObjectClass::Tool,
        "fs.read",
        &target.display().to_string(),
        &[
            ("description", "Read a visible file"),
            ("schema", "{\"type\":\"object\",\"properties\":{}}"),
            ("policy", "allow coder_t tool:fs.read execute"),
        ],
    );
    assert!(tool.is_ok());
    let Ok(tool) = tool else { return };

    assert_eq!(model.executable(), root.join("model").join("debug").join("echo"));
    assert_eq!(tool.control_dir(), root.join("tool").join("fs.read.d"));
    assert!(inspect_object_layout(&root, ObjectClass::Model, "debug/echo").is_ok());
    assert!(inspect_object_layout(&root, ObjectClass::Tool, "fs.read").is_ok());

    let wrapper = fs::read_to_string(root.join("tool").join("fs.read"));
    assert!(wrapper.is_ok());
    let Ok(wrapper) = wrapper else { return };
    assert!(wrapper.starts_with("#!/bin/sh\n"));
    assert!(wrapper.contains("exec '"));
    let permissions = fs::metadata(root.join("tool").join("fs.read"))
        .map(|metadata| metadata.permissions().mode());
    assert!(permissions.is_ok());
    let Ok(permissions) = permissions else {
        return;
    };
    assert_ne!(permissions & 0o111, 0);

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn executable_object_bootstrap_validates_controls_and_agent_socket_boundary() {
    let root = unique_test_dir("object-bootstrap-bad");
    let target = root.join("runtime").join("agent");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_fixture_file(&target, 0o755);

    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "bad/name",
            &target.display().to_string(),
            &[],
        ),
        Err(ObjectBootstrapError::InvalidObjectName)
    );
    assert_eq!(
        install_executable_object_wrapper(&root, ObjectClass::Tool, "fs.read", "bad\ncmd", &[]),
        Err(ObjectBootstrapError::InvalidWrapperTarget)
    );
    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[("authority", "root")],
        ),
        Err(ObjectBootstrapError::InvalidControlFile)
    );
    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[("schema", "{\"authority\":\"root\"}")],
        ),
        Err(ObjectBootstrapError::InvalidControlValue)
    );

    let agent = install_executable_object_wrapper(
        &root,
        ObjectClass::Agent,
        "coder",
        &target.display().to_string(),
        &[("uid", "1000"), ("gid", "1000"), ("owner", "1000")],
    );
    assert!(agent.is_ok());
    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(!report.is_ok());
    assert!(report.issues().contains(&ObjectLayoutIssue::MissingSocket(
        "agent/coder.sock".to_owned()
    )));
    let _agent_socket = bind_socket(&root.join("agent").join("coder.sock"));
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
    assert_eq!(ObjectBootstrapError::InvalidControlValue.errno(), "EINVAL");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn object_layout_accepts_socket_symlink_to_live_unix_socket() {
    let root = unique_test_dir("object-layout-socket-symlink");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let runtime_socket = root.join("runtime").join("coder.sock");
    let _listener = bind_socket(&runtime_socket);
    assert!(symlink(runtime_socket, root.join("agent").join("coder.sock")).is_ok());

    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn object_layout_reports_missing_parts() {
    let root = unique_test_dir("object-layout-bad");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    write_text_file(&root.join("agent").join("coder"), "#!/bin/sh\n");

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(!report.is_ok());
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::NotExecutable("agent/coder".to_owned())));
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::MissingControlDirectory(
            "agent/coder.d".to_owned()
        )));
    assert!(report.issues().contains(&ObjectLayoutIssue::MissingSocket(
        "agent/coder.sock".to_owned()
    )));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn model_session_control_decides_socket_requirement() {
    let root = unique_test_dir("object-layout-model-session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Model, "openai/gpt-4o", "none");

    let no_socket = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(no_socket.is_ok());

    write_text_file(
        &root
            .join("model")
            .join("openai")
            .join("gpt-4o.d")
            .join("session"),
        "socket\n",
    );
    let missing_socket = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(missing_socket
        .issues()
        .contains(&ObjectLayoutIssue::MissingSocket(
            "model/openai/gpt-4o.sock".to_owned()
        )));

    write_text_file(
        &root
            .join("model")
            .join("openai")
            .join("gpt-4o.d")
            .join("session"),
        "native_thread\n",
    );
    let invalid = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(invalid
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/session".to_owned(),
            value: "native_thread".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn model_capabilities_accept_only_stable_words() {
    let valid = inspect_model_capabilities("chat\nstream\ntool_call_syntax\n\n");
    assert!(valid.is_ok());

    let invalid = inspect_model_capabilities("openai_responses\nnative_thread\nvendor_magic\n");
    assert_eq!(
        invalid.issues(),
        &[
            ModelCapabilityIssue::ProviderPrivate {
                line: 1,
                capability: "openai_responses".to_owned()
            },
            ModelCapabilityIssue::ProviderPrivate {
                line: 2,
                capability: "native_thread".to_owned()
            },
            ModelCapabilityIssue::Unknown {
                line: 3,
                capability: "vendor_magic".to_owned()
            }
        ]
    );
}

#[test]
fn model_driver_routes_support_legacy_and_use_case_specific_drivers() {
    let legacy = parse_model_driver_routes("debug\n");
    assert!(legacy.is_ok());
    let Ok(legacy) = legacy else { return };
    assert_eq!(
        legacy.drivers_for(ModelDriverUseCase::Exec),
        Some([String::from("debug")].as_slice())
    );
    assert_eq!(
        legacy.primary_driver_for(ModelDriverUseCase::Agent),
        Some("debug")
    );

    let routed = parse_model_driver_routes(
        "\
default=openai-chat
exec=openai-chat
socket=openai-chat
agent=openai-responses,openai-chat
",
    );
    assert!(routed.is_ok());
    let Ok(routed) = routed else { return };
    assert_eq!(
        routed.drivers_for(ModelDriverUseCase::Exec),
        Some([String::from("openai-chat")].as_slice())
    );
    assert_eq!(
        routed.drivers_for(ModelDriverUseCase::Agent),
        Some([
            String::from("openai-responses"),
            String::from("openai-chat")
        ]
        .as_slice())
    );
    assert_eq!(
        routed.primary_driver_for(ModelDriverUseCase::Socket),
        Some("openai-chat")
    );
}

#[test]
fn model_driver_routes_reject_invalid_route_tables() {
    assert_eq!(
        parse_model_driver_routes("\n# comment\n"),
        Err(ModelDriverRouteError::Empty)
    );
    assert_eq!(
        parse_model_driver_routes("direct=openai-chat\n"),
        Err(ModelDriverRouteError::UnknownUseCase {
            line: 1,
            value: "direct".to_owned()
        })
    );
    assert_eq!(
        parse_model_driver_routes("agent=openai-chat\nagent=openai-responses\n"),
        Err(ModelDriverRouteError::DuplicateUseCase {
            line: 2,
            value: "agent".to_owned()
        })
    );
    assert_eq!(
        parse_model_driver_routes("agent=openai-chat,,openai-responses\n"),
        Err(ModelDriverRouteError::EmptyDriver { line: 1 })
    );
    assert_eq!(
        parse_model_driver_routes("agent=/bin/sh\n"),
        Err(ModelDriverRouteError::InvalidDriverName {
            line: 1,
            value: "/bin/sh".to_owned()
        })
    );
}

#[test]
fn model_object_layout_rejects_provider_private_capabilities() {
    let root = unique_test_dir("object-layout-model-cap");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Model, "openai/gpt-4o", "none");
    write_text_file(
        &root
            .join("model")
            .join("openai")
            .join("gpt-4o.d")
            .join("cap"),
        "chat\nnative_thread\n",
    );

    let report = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/cap".to_owned(),
            value: "native_thread".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn model_object_layout_rejects_invalid_driver_routes() {
    let root = unique_test_dir("object-layout-model-driver");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Model, "openai/gpt-4o", "none");
    write_text_file(
        &root
            .join("model")
            .join("openai")
            .join("gpt-4o.d")
            .join("driver"),
        "agent=/bin/sh\n",
    );

    let report = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/driver".to_owned(),
            value: "line 1 invalid driver /bin/sh".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_schema_accepts_json_schema_shape_without_authority() {
    let report = inspect_tool_schema_json(
        r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn tool_schema_rejects_invalid_json_and_authority_fields() {
    assert_eq!(
        inspect_tool_schema_json("not-json").issues(),
        &[ToolSchemaIssue::InvalidJson]
    );
    assert_eq!(
        inspect_tool_schema_json("[]").issues(),
        &[ToolSchemaIssue::NotObject]
    );
    assert_eq!(
        inspect_tool_schema_json(r#"{"policy":"allow all","permissions":["tool:*"]}"#).issues(),
        &[
            ToolSchemaIssue::AuthorityField("permissions".to_owned()),
            ToolSchemaIssue::AuthorityField("policy".to_owned())
        ]
    );
}

#[test]
fn tool_object_layout_rejects_authority_shaped_schema() {
    let root = unique_test_dir("object-layout-tool-schema");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Tool, "fs.read", "none");
    write_text_file(
        &root.join("tool").join("fs.read.d").join("schema"),
        "{\"policy\":\"allow all\"}\n",
    );

    let report = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "tool/fs.read.d/schema".to_owned(),
            value: "policy".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn agent_controls_accept_fixed_v1_values() {
    assert!(inspect_agent_control(AgentControlKind::Owner, "1000\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Uid, "1000\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Gid, "100\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Groups, "10\n20\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Groups, "").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Iso, "shared\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Iso, "uid\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Life, "owned\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Parent, "\n").is_ok());
    assert!(inspect_agent_control(
        AgentControlKind::Parent,
        "agent:coder session:default run:r1\n"
    )
    .is_ok());
    assert!(inspect_agent_control(AgentControlKind::Status, "idle\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Pid, "\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Pid, "1234\n").is_ok());
}

#[test]
fn agent_controls_reject_invalid_identity_lifecycle_and_parent() {
    assert_eq!(
        inspect_agent_control(AgentControlKind::Uid, "not-a-uid\n").issues(),
        &[AgentControlIssue::InvalidNumber {
            line: 1,
            value: "not-a-uid".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Groups, "10\nbad\n").issues(),
        &[AgentControlIssue::InvalidNumber {
            line: 2,
            value: "bad".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Life, "detached\n").issues(),
        &[AgentControlIssue::InvalidValue {
            line: 1,
            value: "detached".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Parent, "coder session:default\n").issues(),
        &[AgentControlIssue::InvalidValue {
            line: 1,
            value: "coder session:default".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Status, "running\nextra\n").issues(),
        &[
            AgentControlIssue::InvalidValue {
                line: 1,
                value: "running".to_owned()
            },
            AgentControlIssue::MultipleValues { line: 2 }
        ]
    );
}

#[test]
fn agent_object_layout_rejects_invalid_control_values() {
    let root = unique_test_dir("object-layout-agent-controls");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent").join("coder.d");
    write_text_file(&control.join("iso"), "container\n");
    write_text_file(&control.join("uid"), "bad\n");

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "agent/coder.d/iso".to_owned(),
            value: "container".to_owned()
        }));
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "agent/coder.d/uid".to_owned(),
            value: "bad".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn agent_runtime_view_derives_identity_environment_policy_and_view() {
    let root = unique_test_dir("agent-runtime-view");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent").join("coder.d");
    write_text_file(
        &control.join("env"),
        "CTX_ROOT=/ignored\nHOME=/ignored\nRUST_LOG=info\n",
    );

    let view = derive_agent_runtime_view(&root, "coder");
    assert!(view.is_ok());
    let Ok(view) = view else { return };

    assert_eq!(view.agent_name(), "coder");
    assert_eq!(view.control_dir(), control.as_path());
    assert_eq!(view.ctx_root(), root.as_path());
    assert_eq!(view.ctx_home(), root.join("home").join("1000").as_path());
    assert_eq!(
        view.home(),
        root.join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .as_path()
    );
    assert_eq!(view.owner(), 1000);
    assert_eq!(view.identity().uid(), 1000);
    assert_eq!(view.identity().gid(), 100);
    assert_eq!(view.identity().groups(), &[10, 20]);
    assert_eq!(view.label(), "user_u:agent_r:coder_t:s0");
    assert_eq!(view.policy_subject(), "coder_t");
    assert_eq!(view.iso(), "shared");
    assert_eq!(view.parent(), None);
    assert_eq!(view.lifecycle(), ChildLifecycle::Owned);
    assert_eq!(view.root(), Path::new("/ctx/home/1000/agent/coder/root"));
    assert_eq!(view.cwd(), Path::new("/work"));
    assert_eq!(view.model(), "debug/echo");
    assert_eq!(
        view.tool_path().dirs(),
        [
            PathBuf::from("/ctx/tool"),
            PathBuf::from("/ctx/home/1000/tool")
        ]
    );
    assert_eq!(view.mount_table().entries().len(), 1);
    assert!(view.policy().allows(
        "coder_t",
        PolicyObjectClass::Model,
        "debug/echo",
        PolicyPermission::Use,
    ));
    assert_eq!(
        env_value(view.env(), "CTX_ROOT").map(str::to_owned),
        Some(root.display().to_string())
    );
    assert_eq!(
        env_value(view.env(), "CTX_HOME").map(str::to_owned),
        Some(root.join("home").join("1000").display().to_string())
    );
    assert_eq!(
        env_value(view.env(), "HOME").map(str::to_owned),
        Some(
            root.join("home")
                .join("1000")
                .join("agent")
                .join("coder")
                .display()
                .to_string()
        )
    );
    assert_eq!(
        env_value(view.env(), "CTX_PATH"),
        Some("/ctx/tool:/ctx/home/1000/tool")
    );
    assert_eq!(env_value(view.env(), "RUST_LOG"), Some("info"));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn agent_runtime_view_rejects_invalid_control_files() {
    let cases = [
        ("uid", "not-a-uid\n"),
        ("groups", "10\nbad\n"),
        ("label", "user_u:agent_r:bad/name:s0\n"),
        ("root", "../root\n"),
        ("cwd", "/work/../secret\n"),
        ("env", "1BAD=value\n"),
        ("path", "/ctx/tool:../tool\n"),
        ("mount", "bad\n"),
        ("model", "bad/name/extra\n"),
        ("policy", "allow bad\n"),
    ];

    for (file, value) in cases {
        let root = unique_test_dir(&format!("agent-runtime-invalid-{file}"));
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
        write_text_file(&root.join("agent").join("coder.d").join(file), value);

        assert_eq!(
            derive_agent_runtime_view(&root, "coder"),
            Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
        );
        assert_eq!(
            AgentRuntimeViewError::InvalidControlFile(file.to_owned()).errno(),
            "EINVAL"
        );

        let _ignored = fs::remove_dir_all(&root);
    }
}

#[test]
fn agent_runtime_view_reports_missing_controls_and_bad_agent_names() {
    let root = unique_test_dir("agent-runtime-missing");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    assert_eq!(
        derive_agent_runtime_view(&root, "bad/name"),
        Err(AgentRuntimeViewError::InvalidAgentName)
    );

    let model = root.join("agent").join("coder.d").join("model");
    assert!(fs::remove_file(model).is_ok());
    assert_eq!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::MissingControlFile(
            "model".to_owned()
        ))
    );
    assert_eq!(
        AgentRuntimeViewError::MissingControlFile("model".to_owned()).errno(),
        "ENOENT"
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn agent_runtime_view_env_prompt_and_skill_text_do_not_expand_tool_path() {
    let root = unique_test_dir("agent-runtime-no-text-grant");
    let allowed = root.join("tool");
    let env_only = root.join("env-tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    write_fixture_file(&env_only.join("fs.read"), 0o755);
    write_text_file(
        &root.join("work").join("AGENTS.md"),
        "The agent may execute fs.read.\n",
    );
    write_text_file(
        &root.join("work").join(".mcp.json"),
        "{\"servers\":{\"fs\":{\"tools\":[\"fs.read\"]}}}\n",
    );

    let control = root.join("agent").join("coder.d");
    write_text_file(&control.join("path"), &format!("{}\n", allowed.display()));
    write_text_file(
        &control.join("env"),
        &format!("CTX_PATH={}\nAGENT_RULES=allow\n", env_only.display()),
    );
    write_text_file(
        &control.join("policy"),
        "allow coder_t tool:fs.read execute\n",
    );

    let view = derive_agent_runtime_view(&root, "coder");
    assert!(view.is_ok());
    let Ok(view) = view else { return };
    assert_eq!(
        env_value(view.env(), "CTX_PATH").map(str::to_owned),
        Some(allowed.display().to_string())
    );
    assert_eq!(env_value(view.env(), "AGENT_RULES"), Some("allow"));

    let metadata = fs::metadata(env_only.join("fs.read"));
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_target(&env_only, "rw", "bind,nosuid,nodev");
    let tool_policy = allow_tool_policy("coder_t", "fs.read");
    let denied = authorize_tool_execution(
        view.tool_path(),
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &mounts,
            view.policy_subject(),
            view.policy(),
            &tool_policy,
        ),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::ToolNotFound));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn api_key_resolution_prefers_environment_over_keychain() {
    let resolved = resolve_api_key_with(
        "LMM_API_KEY",
        "cortexfs:lmm",
        "default",
        |_name| Ok("env-secret".to_owned()),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(resolved, Ok(Some("env-secret".to_owned())));
}

#[test]
fn api_key_resolution_uses_keychain_when_environment_is_empty_or_missing() {
    let empty_env = resolve_api_key_with(
        "LMM_API_KEY",
        "cortexfs:lmm",
        "default",
        |_name| Ok(" \n".to_owned()),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(empty_env, Ok(Some("keychain-secret".to_owned())));

    let missing_env = resolve_api_key_with(
        "LMM_API_KEY",
        "cortexfs:lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(missing_env, Ok(Some("keychain-secret".to_owned())));
}

#[test]
fn api_key_resolution_reports_unconfigured_without_environment_or_keychain() {
    let resolved = resolve_api_key_with(
        "LMM_API_KEY",
        "cortexfs:lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(None),
    );
    assert_eq!(resolved, Ok(None));

    let invalid = resolve_api_key_with(
        "BAD-NAME",
        "cortexfs:lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(None),
    );
    assert_eq!(invalid, Err(ApiKeyResolutionError::InvalidName));
}

#[test]
fn socket_peer_credentials_come_from_kernel() {
    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((left, right)) = pair else { return };

    let left_peer = peer_credentials(&left);
    let right_peer = peer_credentials(&right);
    assert!(left_peer.is_ok());
    assert!(right_peer.is_ok());
    let Ok(left_peer) = left_peer else { return };
    let Ok(right_peer) = right_peer else { return };

    assert_eq!(left_peer.uid(), right_peer.uid());
    assert_eq!(left_peer.gid(), right_peer.gid());
    assert!(left_peer.pid().is_some());
    assert!(SocketPeerPolicy::uid(left_peer.uid()).allows(left_peer));
    assert!(SocketPeerPolicy::gid(left_peer.gid()).allows(left_peer));
    assert!(SocketPeerPolicy::uid_gid(left_peer.uid(), left_peer.gid()).allows(left_peer));
}

#[test]
fn socket_peer_policy_rejects_mismatched_identity() {
    let peer = PeerCredentials::new(Some(1), 1000, 100);
    assert!(SocketPeerPolicy::uid(1000).allows(peer));
    assert!(SocketPeerPolicy::gid(100).allows(peer));
    assert!(SocketPeerPolicy::uid_gid(1000, 100).allows(peer));
    assert!(!SocketPeerPolicy::uid(1001).allows(peer));
    assert!(!SocketPeerPolicy::gid(101).allows(peer));
    assert!(!SocketPeerPolicy::uid_gid(1000, 101).allows(peer));
}

#[test]
fn socket_request_parser_accepts_stable_request_frames() {
    assert_eq!(
        parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","session":"default","scope":"shared","cwd":"/work","input":"hello","thread_id":"ignored"}
"#
        ),
        Ok(SocketRequest::Send {
            id: "msg-1".to_owned(),
            session: "default".to_owned(),
            scope: SocketSessionScope::Shared,
            cwd: Some("/work".to_owned()),
            input: "hello".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"resume","session":"default","after":"event-123"}"#),
        Ok(SocketRequest::Resume {
            session: "default".to_owned(),
            after: Some("event-123".to_owned())
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#),
        Ok(SocketRequest::Cancel {
            id: "run-1".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"ping"}"#),
        Ok(SocketRequest::Ping)
    );
}

#[test]
fn socket_request_parser_defaults_session_and_scope() {
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"send","id":"msg-1","input":"hello"}"#),
        Ok(SocketRequest::Send {
            id: "msg-1".to_owned(),
            session: "default".to_owned(),
            scope: SocketSessionScope::Private,
            cwd: None,
            input: "hello".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"resume"}"#),
        Ok(SocketRequest::Resume {
            session: "default".to_owned(),
            after: None
        })
    );
    assert_eq!(SocketSessionScope::Temp.as_str(), "temp");
}

#[test]
fn socket_request_parser_reports_stable_errno_for_bad_frames() {
    let oversized = "x".repeat(MAX_SOCKET_FRAME_BYTES + 1);
    let error = parse_socket_request_frame(&oversized);
    assert!(matches!(
        error,
        Err(SocketRequestError::FrameTooLarge { bytes }) if bytes == MAX_SOCKET_FRAME_BYTES + 1
    ));
    assert_eq!(
        error.err().as_ref().map(SocketRequestError::errno),
        Some("EMSGSIZE")
    );

    let invalid = parse_socket_request_frame("{}");
    assert_eq!(invalid, Err(SocketRequestError::MissingOp));
    assert_eq!(
        invalid.err().as_ref().map(SocketRequestError::errno),
        Some("EINVAL")
    );
}

#[test]
fn socket_request_parser_rejects_invalid_ops_and_fields() {
    assert_eq!(
        parse_socket_request_frame(""),
        Err(SocketRequestError::EmptyFrame)
    );
    assert_eq!(
        parse_socket_request_frame("{\"op\":\"ping\"}\n{\"op\":\"ping\"}\n"),
        Err(SocketRequestError::MultipleFrames)
    );
    assert_eq!(
        parse_socket_request_frame("[1]"),
        Err(SocketRequestError::RequestNotObject)
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"native_thread"}"#),
        Err(SocketRequestError::UnknownOp("native_thread".to_owned()))
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"send","id":"bad/id","input":"hello"}"#),
        Err(SocketRequestError::InvalidField {
            field: "id",
            value: "bad/id".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","scope":"global","input":"hello"}"#
        ),
        Err(SocketRequestError::InvalidField {
            field: "scope",
            value: "global".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","cwd":"/work/../secret","input":"hello"}"#
        ),
        Err(SocketRequestError::InvalidField {
            field: "cwd",
            value: "/work/../secret".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"send","id":"msg-1","input":42}"#),
        Err(SocketRequestError::MissingStringField("input"))
    );
}

#[test]
fn socket_session_recorder_appends_send_to_durable_history() {
    let root = unique_test_dir("socket-session-send");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    assert!(request.is_ok());
    let Ok(request) = request else { return };
    let recorded = record_socket_request_to_session(&session, &request);
    assert!(recorded.is_ok());
    let Ok(recorded) = recorded else { return };
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 1);

    let messages = fs::read_to_string(session.join("messages.jsonl"));
    assert!(messages.is_ok());
    let Ok(messages) = messages else { return };
    let events = fs::read_to_string(session.join("events.jsonl"));
    assert!(events.is_ok());
    let Ok(events) = events else { return };
    assert!(inspect_message_stream_jsonl(&messages).is_ok());
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(messages.contains("\"role\":\"user\""));
    assert!(messages.contains("\"content\":\"hello\""));
    assert!(events.contains("\"type\":\"start\""));
    let state = fs::read_to_string(session.join("state"));
    assert!(state.is_ok());
    let Ok(state) = state else { return };
    assert_eq!(state, "active\n");
    let cwd = fs::read_to_string(session.join("cwd"));
    assert!(cwd.is_ok());
    let Ok(cwd) = cwd else { return };
    assert_eq!(cwd, "/work/project\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_session_recorder_cancels_without_deleting_history() {
    let root = unique_test_dir("socket-session-cancel");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"keep me\"}\n",
    );
    write_text_file(&session.join("events.jsonl"), "");

    let request = parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#);
    assert!(request.is_ok());
    let Ok(request) = request else { return };
    let recorded = record_socket_request_to_session(&session, &request);
    assert!(recorded.is_ok());
    let Ok(recorded) = recorded else { return };
    assert!(recorded.messages().is_empty());
    assert_eq!(recorded.events().len(), 1);

    let messages = fs::read_to_string(session.join("messages.jsonl"));
    assert!(messages.is_ok());
    let Ok(messages) = messages else { return };
    let events = fs::read_to_string(session.join("events.jsonl"));
    assert!(events.is_ok());
    let Ok(events) = events else { return };
    assert_eq!(messages, "{\"role\":\"user\",\"content\":\"keep me\"}\n");
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(events.contains("\"status\":\"cancelled\""));
    let state = fs::read_to_string(session.join("state"));
    assert!(state.is_ok());
    let Ok(state) = state else { return };
    assert_eq!(state, "cancelled\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn assistant_response_recorder_updates_latest_without_replacing_history() {
    let root = unique_test_dir("assistant-response-record");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"hello\"}\n",
    );
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
    );
    write_text_file(&session.join("latest.md"), "old\n");

    let recorded = record_assistant_response_to_session(&session, "run-1", "hello back");
    assert!(recorded.is_ok());
    let Ok(recorded) = recorded else { return };
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 2);

    let messages = fs::read_to_string(session.join("messages.jsonl"));
    assert!(messages.is_ok());
    let Ok(messages) = messages else { return };
    let events = fs::read_to_string(session.join("events.jsonl"));
    assert!(events.is_ok());
    let Ok(events) = events else { return };
    let latest = fs::read_to_string(session.join("latest.md"));
    assert!(latest.is_ok());
    let Ok(latest) = latest else { return };
    let state = fs::read_to_string(session.join("state"));
    assert!(state.is_ok());
    let Ok(state) = state else { return };

    assert!(inspect_message_stream_jsonl(&messages).is_ok());
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(messages.contains("\"role\":\"user\""));
    assert!(messages.contains("\"role\":\"assistant\""));
    assert!(events.contains("\"type\":\"message\""));
    assert!(events.contains("\"status\":\"ok\""));
    assert_eq!(latest, "hello back\n");
    assert_eq!(state, "done\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_denial_recorder_makes_permission_failure_inspectable() {
    let root = unique_test_dir("tool-denial-record");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
    );

    let recorded = record_tool_execution_denial_to_session(
        &session,
        "run-1",
        "fs.read",
        ToolExecutionDenial::AgentPolicy,
    );
    assert!(recorded.is_ok());
    let Ok(recorded) = recorded else { return };
    assert!(recorded.messages().is_empty());
    assert_eq!(recorded.events().len(), 2);

    let events = fs::read_to_string(session.join("events.jsonl"));
    assert!(events.is_ok());
    let Ok(events) = events else { return };
    let state = fs::read_to_string(session.join("state"));
    assert!(state.is_ok());
    let Ok(state) = state else { return };

    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(events.contains("\"type\":\"error\""));
    assert!(events.contains("\"tool\":\"fs.read\""));
    assert!(events.contains("\"code\":\"EACCES\""));
    assert!(events.contains("\"status\":\"error\""));
    assert_eq!(state, "error\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_denial_recorder_rejects_invalid_tool_names() {
    let root = unique_test_dir("tool-denial-record-bad");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);

    assert_eq!(
        record_tool_execution_denial_to_session(
            &session,
            "run-1",
            "bad/tool",
            ToolExecutionDenial::InvalidToolName,
        ),
        Err(SocketSessionRecordError::InvalidField("tool"))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_result_recorder_appends_inspectable_tool_message_and_event() {
    let root = unique_test_dir("tool-result-record");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"read README\"}\n",
    );
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
    );

    let recorded = record_tool_execution_result_to_session(
        &session,
        "run-1",
        "call-1",
        "fs.read",
        "file contents",
    );
    assert!(recorded.is_ok());
    let Ok(recorded) = recorded else { return };
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 1);

    let messages = fs::read_to_string(session.join("messages.jsonl"));
    assert!(messages.is_ok());
    let Ok(messages) = messages else { return };
    let events = fs::read_to_string(session.join("events.jsonl"));
    assert!(events.is_ok());
    let Ok(events) = events else { return };

    assert!(inspect_message_stream_jsonl(&messages).is_ok());
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(messages.contains("\"role\":\"tool\""));
    assert!(messages.contains("\"type\":\"tool_result\""));
    assert!(messages.contains("\"tool_call_id\":\"call-1\""));
    assert!(events.contains("\"name\":\"fs.read\""));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_result_recorder_rejects_invalid_fields_without_executing() {
    let root = unique_test_dir("tool-result-record-bad");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);

    assert_eq!(
        record_tool_execution_result_to_session(&session, "run-1", "call-1", "bad/tool", "content",),
        Err(SocketSessionRecordError::InvalidField("tool"))
    );
    assert_eq!(
        record_tool_execution_result_to_session(
            &session,
            "run-1",
            "call-1",
            "fs.read",
            "bad\0content",
        ),
        Err(SocketSessionRecordError::InvalidField("content"))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_session_recorder_rejects_temp_resume_and_mismatched_sessions() {
    let root = unique_test_dir("socket-session-reject");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);

    let temp = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","scope":"temp","input":"hello"}"#,
    );
    assert!(temp.is_ok());
    let Ok(temp) = temp else { return };
    assert_eq!(
        record_socket_request_to_session(&session, &temp),
        Err(SocketSessionRecordError::TempSessionNotDurable)
    );

    let resume = parse_socket_request_frame(r#"{"op":"resume","session":"default"}"#);
    assert!(resume.is_ok());
    let Ok(resume) = resume else { return };
    assert_eq!(
        record_socket_request_to_session(&session, &resume),
        Err(SocketSessionRecordError::UnsupportedRequest)
    );

    let mismatch = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-2","session":"other","input":"hello"}"#,
    );
    assert!(mismatch.is_ok());
    let Ok(mismatch) = mismatch else { return };
    assert_eq!(
        record_socket_request_to_session(&session, &mismatch),
        Err(SocketSessionRecordError::SessionMismatch)
    );
    assert_eq!(SocketSessionRecordError::SessionMismatch.errno(), "EINVAL");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn indexed_socket_send_records_history_and_updates_session_index() {
    let root = unique_test_dir("indexed-socket-send");
    let session_root = root.join("session");
    let session = session_root.join("default");
    let previous = session_root.join("review-1");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    create_complete_session_layout(&previous);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(
        &session_root.join("index").join("list"),
        "review-1\ndefault\n",
    );
    write_text_file(&session_root.join("index").join("current"), "review-1\n");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    assert!(request.is_ok());
    let Ok(request) = request else { return };
    let recorded = record_indexed_socket_send_to_session(&session_root, &request);
    assert!(recorded.is_ok());
    let Ok(recorded) = recorded else { return };
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 1);

    let by_cwd_key = session_index_key_for_cwd("/work/project");
    assert!(by_cwd_key.is_some());
    let Some(by_cwd_key) = by_cwd_key else { return };
    let messages = fs::read_to_string(session.join("messages.jsonl"));
    assert!(messages.is_ok());
    let Ok(messages) = messages else { return };
    let events = fs::read_to_string(session.join("events.jsonl"));
    assert!(events.is_ok());
    let Ok(events) = events else { return };
    let list = fs::read_to_string(session_root.join("index").join("list"));
    assert!(list.is_ok());
    let Ok(list) = list else { return };
    let current = fs::read_to_string(session_root.join("index").join("current"));
    assert!(current.is_ok());
    let Ok(current) = current else { return };
    let by_cwd = fs::read_to_string(session_root.join("index").join("by-cwd").join(by_cwd_key));
    assert!(by_cwd.is_ok());
    let Ok(by_cwd) = by_cwd else { return };

    assert!(messages.contains("\"role\":\"user\""));
    assert!(events.contains("\"type\":\"start\""));
    assert_eq!(list, "default\nreview-1\n");
    assert_eq!(current, "default\n");
    assert_eq!(by_cwd, "default\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn indexed_socket_send_rejects_non_send_requests() {
    let root = unique_test_dir("indexed-socket-non-send");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

    let resume = parse_socket_request_frame(r#"{"op":"resume","session":"default"}"#);
    assert!(resume.is_ok());
    let Ok(resume) = resume else { return };
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &resume),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::UnsupportedRequest
        ))
    );

    let cancel = parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#);
    assert!(cancel.is_ok());
    let Ok(cancel) = cancel else { return };
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &cancel),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::UnsupportedRequest
        ))
    );

    let ping = parse_socket_request_frame(r#"{"op":"ping"}"#);
    assert!(ping.is_ok());
    let Ok(ping) = ping else { return };
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &ping),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::UnsupportedRequest
        ))
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn indexed_socket_send_rejects_temp_sessions_before_index_update() {
    let root = unique_test_dir("indexed-socket-temp");
    let session_root = root.join("session");
    let session = session_root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    write_text_file(&session_root.join("index").join("list"), "default\n");
    write_text_file(&session_root.join("index").join("current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

    let temp = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","scope":"temp","cwd":"/work","input":"hello"}"#,
    );
    assert!(temp.is_ok());
    let Ok(temp) = temp else { return };
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &temp),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::TempSessionNotDurable
        ))
    );
    let list = fs::read_to_string(session_root.join("index").join("list"));
    assert!(list.is_ok());
    let Ok(list) = list else { return };
    assert_eq!(list, "default\n");
    assert!(!session_root
        .join("index")
        .join("by-cwd")
        .join("cwd")
        .exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn durable_session_layout_helper_creates_inspectable_session_and_index() {
    let root = unique_test_dir("durable-session-layout");
    let session_root = root.join("session");
    let session = session_root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

    let ensured = ensure_durable_session_layout(
        &session_root,
        "default",
        "/work/project",
        Some("debug/echo"),
        SocketSessionScope::Private,
    );
    assert_eq!(ensured, Ok(()));
    assert!(inspect_session_layout(&session).is_ok());

    let list = fs::read_to_string(session_root.join("index").join("list"));
    assert!(list.is_ok());
    let Ok(list) = list else { return };
    let current = fs::read_to_string(session_root.join("index").join("current"));
    assert!(current.is_ok());
    let Ok(current) = current else { return };
    let meta = fs::read_to_string(session.join("meta.json"));
    assert!(meta.is_ok());
    let Ok(meta) = meta else { return };
    let pack = fs::read_to_string(session.join("context").join("pack.json"));
    assert!(pack.is_ok());
    let Ok(pack) = pack else { return };

    assert_eq!(list, "default\n");
    assert_eq!(current, "default\n");
    assert!(meta.contains("\"model\":\"debug/echo\""));
    assert!(meta.contains("\"scope\":\"private\""));
    assert!(inspect_context_pack_json(&pack).is_ok());

    let updated = ensure_durable_session_layout(
        &session_root,
        "default",
        "/work/project",
        Some("openai/gpt-4o"),
        SocketSessionScope::Private,
    );
    assert_eq!(updated, Ok(()));
    let meta = fs::read_to_string(session.join("meta.json"));
    assert!(matches!(meta, Ok(ref meta) if meta.contains("\"model\":\"openai/gpt-4o\"")));

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    assert!(request.is_ok());
    let Ok(request) = request else { return };
    assert!(record_indexed_socket_send_to_session(&session_root, &request).is_ok());
    let state = fs::read_to_string(session.join("state"));
    assert!(state.is_ok());
    let Ok(state) = state else { return };
    assert_eq!(state, "active\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn durable_session_layout_helper_rejects_invalid_durable_inputs() {
    let root = unique_test_dir("durable-session-layout-invalid");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

    assert_eq!(
        ensure_durable_session_layout(
            &session_root,
            "bad/name",
            "/work",
            None,
            SocketSessionScope::Private,
        ),
        Err(DurableSessionLayoutError::InvalidSessionName)
    );
    assert_eq!(
        ensure_durable_session_layout(
            &session_root,
            "default",
            "../host",
            None,
            SocketSessionScope::Private,
        ),
        Err(DurableSessionLayoutError::InvalidCwd)
    );
    assert_eq!(
        ensure_durable_session_layout(
            &session_root,
            "default",
            "/work",
            Some("bad/model/extra"),
            SocketSessionScope::Private,
        ),
        Err(DurableSessionLayoutError::InvalidModelName)
    );
    assert_eq!(
        ensure_durable_session_layout(
            &session_root,
            "default",
            "/work",
            None,
            SocketSessionScope::Temp,
        ),
        Err(DurableSessionLayoutError::TempSessionNotDurable)
    );
    assert_eq!(DurableSessionLayoutError::InvalidCwd.errno(), "EINVAL");
    assert!(!session_root.exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_runtime_handles_ping_send_resume_and_cancel() {
    let root = unique_test_dir("socket-runtime");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

    let ping =
        handle_socket_request_frame(&session_root, "/work", Some("debug/echo"), r#"{"op":"ping"}"#);
    assert!(ping.is_ok());
    let Ok(ping) = ping else { return };
    assert_eq!(ping.jsonl(), "{\"type\":\"pong\"}\n");

    let send = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-1","session":"default","input":"hello"}"#,
    );
    assert!(send.is_ok());
    let Ok(send) = send else { return };
    assert_eq!(send.frames().len(), 1);
    assert!(send.jsonl().contains("\"type\":\"start\""));
    assert!(send.jsonl().contains("\"run\":\"msg-1\""));
    assert!(inspect_session_layout(&session_root.join("default")).is_ok());

    let second = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-2","session":"default","input":"again"}"#,
    );
    assert!(second.is_ok());

    let resume_all = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"resume","session":"default"}"#,
    );
    assert!(resume_all.is_ok());
    let Ok(resume_all) = resume_all else { return };
    assert_eq!(resume_all.frames().len(), 2);
    assert!(resume_all.jsonl().contains("\"run\":\"msg-1\""));
    assert!(resume_all.jsonl().contains("\"run\":\"msg-2\""));

    let resume_after = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"resume","session":"default","after":"msg-1"}"#,
    );
    assert!(resume_after.is_ok());
    let Ok(resume_after) = resume_after else {
        return;
    };
    assert_eq!(resume_after.frames().len(), 1);
    assert!(!resume_after.jsonl().contains("\"run\":\"msg-1\""));
    assert!(resume_after.jsonl().contains("\"run\":\"msg-2\""));

    let cancel = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"cancel","id":"msg-2"}"#,
    );
    assert!(cancel.is_ok());
    let Ok(cancel) = cancel else { return };
    assert!(cancel.jsonl().contains("\"status\":\"cancelled\""));
    let state = fs::read_to_string(session_root.join("default").join("state"));
    assert!(state.is_ok());
    let Ok(state) = state else { return };
    assert_eq!(state, "cancelled\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_runtime_temp_send_does_not_create_durable_session() {
    let root = unique_test_dir("socket-runtime-temp");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

    let send = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-1","session":"scratch","scope":"temp","input":"hello"}"#,
    );
    assert!(send.is_ok());
    let Ok(send) = send else { return };
    assert_eq!(send.frames().len(), 1);
    assert!(send.jsonl().contains("\"type\":\"start\""));
    assert!(send.jsonl().contains("\"model\":\"debug/echo\""));
    assert!(!session_root.exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_runtime_errors_convert_to_stable_error_frames() {
    let root = unique_test_dir("socket-runtime-error");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

    let error = handle_socket_request_frame(
        &session_root,
        "/work/../bad",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-1","session":"default","input":"hello"}"#,
    );
    assert_eq!(
        error,
        Err(SocketRuntimeError::SessionLayout(
            DurableSessionLayoutError::InvalidCwd
        ))
    );
    let Err(error) = error else { return };
    let response = socket_runtime_error_response(&error);
    assert_eq!(
        response.jsonl(),
        "{\"code\":\"EINVAL\",\"message\":\"EINVAL\",\"type\":\"error\"}\n"
    );
    let Some(frame) = response.frames().first() else {
        return;
    };
    let parsed = serde_json::from_str::<serde_json::Value>(frame);
    assert!(parsed.is_ok());
    let Ok(parsed) = parsed else { return };
    assert_eq!(
        parsed.get("type").and_then(serde_json::Value::as_str),
        Some("error")
    );
    assert_eq!(
        parsed.get("code").and_then(serde_json::Value::as_str),
        Some("EINVAL")
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_stream_runtime_serves_one_frame_with_peer_credentials() {
    let root = unique_test_dir("socket-stream-runtime");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((mut client, mut socket)) = pair else {
        return;
    };
    let peer = peer_credentials(&socket);
    assert!(peer.is_ok());
    let Ok(peer) = peer else { return };
    let policy = SocketPeerPolicy::uid_gid(peer.uid(), peer.gid());

    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_unix_socket_stream_once(
        &mut socket,
        Some(policy),
        &session_root,
        "/work",
        Some("debug/echo"),
    );
    assert!(outcome.is_ok());
    let Ok(outcome) = outcome else { return };
    assert_eq!(outcome.frames().len(), 1);

    let mut buffer = [0_u8; 256];
    let read = client.read(&mut buffer);
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"start\""));
    assert!(response.contains("\"run\":\"msg-1\""));
    assert!(inspect_session_layout(&session_root.join("default")).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_stream_runtime_denies_wrong_peer_before_mutating_session() {
    let root = unique_test_dir("socket-stream-runtime-deny");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((mut client, mut socket)) = pair else {
        return;
    };
    let peer = peer_credentials(&socket);
    assert!(peer.is_ok());
    let Ok(peer) = peer else { return };
    let denied_uid = if peer.uid() == u32::MAX {
        peer.uid() - 1
    } else {
        peer.uid() + 1
    };
    let policy = SocketPeerPolicy::uid(denied_uid);

    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_unix_socket_stream_once(
        &mut socket,
        Some(policy),
        &session_root,
        "/work",
        Some("debug/echo"),
    );
    assert_eq!(outcome, Err(SocketRuntimeError::PeerDenied));

    let mut buffer = [0_u8; 256];
    let read = client.read(&mut buffer);
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"error\""));
    assert!(response.contains("\"code\":\"EACCES\""));
    assert!(!session_root.exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_listener_runtime_accepts_and_serves_one_connection() {
    let root = unique_test_dir("socket-listener-runtime");
    let session_root = root.join("session");
    let socket_path = root.join("agent.sock");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(&root).is_ok());
    let listener = UnixListener::bind(&socket_path);
    assert!(listener.is_ok());
    let Ok(listener) = listener else { return };

    let client = UnixStream::connect(&socket_path);
    assert!(client.is_ok());
    let Ok(mut client) = client else { return };
    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome =
        serve_unix_socket_listener_once(&listener, None, &session_root, "/work", Some("debug/echo"));
    assert!(outcome.is_ok());
    let Ok(outcome) = outcome else { return };
    assert_eq!(outcome.frames().len(), 1);

    let mut buffer = [0_u8; 256];
    let read = client.read(&mut buffer);
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"start\""));
    assert!(response.contains("\"run\":\"msg-1\""));
    assert!(inspect_session_layout(&session_root.join("default")).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn agent_executable_socket_runtime_returns_visible_message() {
    let root = unique_test_dir("agent-executable-socket-runtime");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session_root = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session");
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
printf '{"type":"start","run":"%s","model":"debug/echo"}\n' "$run"
printf '{"type":"delta","run":"%s","text":"%s"}\n' "$run" "$input"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
"#,
    );
    let permissions = fs::metadata(&agent_executable);
    assert!(permissions.is_ok());
    let Ok(metadata) = permissions else { return };
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    assert!(fs::set_permissions(&agent_executable, permissions).is_ok());
    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((mut client, mut socket)) = pair else {
        return;
    };

    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            agent_name: "coder",
            agent_executable: &agent_executable,
        },
    );
    assert!(outcome.is_ok());
    let Ok(outcome) = outcome else { return };
    assert_eq!(outcome.frames().len(), 3);
    assert!(outcome.jsonl().contains("\"type\":\"start\""));
    assert!(outcome.jsonl().contains("\"type\":\"delta\""));
    assert!(outcome.jsonl().contains("\"text\":\"hi\""));
    assert!(outcome.jsonl().contains("\"type\":\"done\""));

    let mut buffer = [0_u8; 512];
    let read = client.read(&mut buffer);
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"delta\""));
    assert!(response.contains("\"text\":\"hi\""));
    let latest = fs::read_to_string(session_root.join("default").join("latest.md"));
    assert!(latest.is_ok());
    assert_eq!(latest.unwrap_or_default(), "hi\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn agent_executable_socket_runtime_preserves_jsonl_error_output() {
    let root = unique_test_dir("agent-executable-socket-runtime-error-output");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session_root = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session");
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"error","run":"%s","code":"EHOSTDOWN","message":"model unavailable"}\n' "$CTX_RUN_ID"
printf '{"type":"done","run":"%s","status":"error"}\n' "$CTX_RUN_ID"
exit 1
"#,
    );
    let permissions = fs::metadata(&agent_executable).map(|metadata| metadata.permissions());
    assert!(permissions.is_ok());
    let Ok(mut permissions) = permissions else {
        return;
    };
    permissions.set_mode(0o755);
    assert!(fs::set_permissions(&agent_executable, permissions).is_ok());

    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((mut client, mut socket)) = pair else {
        return;
    };

    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            agent_name: "coder",
            agent_executable: &agent_executable,
        },
    );
    assert!(outcome.is_ok());
    let Ok(outcome) = outcome else { return };
    assert!(outcome.jsonl().contains("\"code\":\"EHOSTDOWN\""));
    assert!(outcome
        .jsonl()
        .contains("\"message\":\"model unavailable\""));
    assert!(outcome.jsonl().contains("\"status\":\"error\""));

    let mut buffer = [0_u8; 512];
    let read = client.read(&mut buffer);
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"code\":\"EHOSTDOWN\""));
    assert!(response.contains("\"message\":\"model unavailable\""));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn policy_v0_allows_only_exact_rules() {
    let parsed = PolicyV0::parse(
        "\
allow coder_t tool:fs.read execute
allow coder_t model:debug/echo use
allow coder_t shared:project-a read
",
    );
    assert!(parsed.is_ok());
    let Ok(policy) = parsed else { return };

    assert!(policy.allows(
        "coder_t",
        PolicyObjectClass::Tool,
        "fs.read",
        PolicyPermission::Execute
    ));
    assert!(policy.allows(
        "coder_t",
        PolicyObjectClass::Model,
        "debug/echo",
        PolicyPermission::Use
    ));
    assert!(!policy.allows(
        "coder_t",
        PolicyObjectClass::Tool,
        "shell.exec",
        PolicyPermission::Execute
    ));
    assert!(!policy.allows(
        "reviewer_t",
        PolicyObjectClass::Tool,
        "fs.read",
        PolicyPermission::Execute
    ));
    assert!(!policy.allows(
        "coder_t",
        PolicyObjectClass::Shared,
        "project-a",
        PolicyPermission::Write
    ));
}

#[test]
fn policy_v0_checks_child_authority_subset() {
    let parent = PolicyV0::parse(
        "\
allow coder_t tool:fs.read execute
allow coder_t model:debug/echo use
allow coder_t shared:project-a read
allow coder_t session:default resume
",
    );
    assert!(parent.is_ok());
    let Ok(parent) = parent else { return };

    let child = PolicyV0::parse(
        "\
allow reviewer_t tool:fs.read execute
allow reviewer_t model:debug/echo use
allow reviewer_t shared:project-a read
",
    );
    assert!(child.is_ok());
    let Ok(child) = child else { return };
    assert!(child.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));
    assert!(!child.is_exact_subset_of(&parent));

    let expanded_tool = PolicyV0::parse(
        "\
allow reviewer_t tool:shell.exec execute
",
    );
    assert!(expanded_tool.is_ok());
    let Ok(expanded_tool) = expanded_tool else {
        return;
    };
    assert!(!expanded_tool.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));

    let wrong_subject = PolicyV0::parse(
        "\
allow other_t tool:fs.read execute
",
    );
    assert!(wrong_subject.is_ok());
    let Ok(wrong_subject) = wrong_subject else {
        return;
    };
    assert!(!wrong_subject.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));
}

#[test]
fn policy_v0_rejects_invalid_rules() {
    assert_eq!(
        PolicyRule::parse("deny coder_t tool:fs.read execute"),
        Err(PolicyError::ExpectedAllow)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t provider:openai use"),
        Err(PolicyError::UnknownClass)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t tool:fs.read use"),
        Err(PolicyError::UnknownPermission)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t tool:* execute"),
        Err(PolicyError::InvalidName)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t tool:fs.read execute extra"),
        Err(PolicyError::WrongFieldCount)
    );
}

#[test]
fn mount_table_parses_fixed_v0_format() {
    let parsed = MountTable::parse(
        "\
/ctx\t/ctx\tro\trbind,nosuid,nodev,noexec
/home/me/project\t/work\trw\trbind,nosuid,nodev
/tmp\t/tmp\trw\t-
",
    );
    assert!(parsed.is_ok());
    let Ok(table) = parsed else { return };
    assert_eq!(table.entries().len(), 3);

    let Some(first) = table.entries().first() else {
        return;
    };
    assert_eq!(first.source(), "/ctx");
    assert_eq!(first.target(), "/ctx");
    assert_eq!(first.mode(), MountMode::ReadOnly);
    assert_eq!(
        first.options(),
        [
            MountOption::RecursiveBind,
            MountOption::NoSuid,
            MountOption::NoDev,
            MountOption::NoExec
        ]
    );

    let Some(last) = table.entries().last() else {
        return;
    };
    assert!(last.options().is_empty());
}

#[test]
fn mount_table_checks_child_attenuation() {
    let parent = MountTable::parse(
        "\
/home/me/project\t/work\trw\trbind,nosuid,nodev
/ctx/shared/project-a\t/shared/project-a\tro\trbind,nosuid,nodev,noexec
",
    );
    assert!(parent.is_ok());
    let Ok(parent) = parent else { return };

    let narrowed = MountTable::parse(
        "\
/home/me/project\t/work\tro\tbind,nosuid,nodev,noexec
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev,noexec
",
    );
    assert!(narrowed.is_ok());
    let Ok(narrowed) = narrowed else { return };
    assert!(narrowed.is_subset_of(&parent));

    let write_expansion = MountTable::parse(
        "\
/ctx/shared/project-a\t/shared/project-a\trw\tbind,nosuid,nodev,noexec
",
    );
    assert!(write_expansion.is_ok());
    let Ok(write_expansion) = write_expansion else {
        return;
    };
    assert!(!write_expansion.is_subset_of(&parent));

    let removed_safety = MountTable::parse(
        "\
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev
",
    );
    assert!(removed_safety.is_ok());
    let Ok(removed_safety) = removed_safety else {
        return;
    };
    assert!(!removed_safety.is_subset_of(&parent));

    let hidden_parent_path = MountTable::parse(
        "\
/secret\t/secret\tro\tbind,nosuid,nodev,noexec
",
    );
    assert!(hidden_parent_path.is_ok());
    let Ok(hidden_parent_path) = hidden_parent_path else {
        return;
    };
    assert!(!hidden_parent_path.is_subset_of(&parent));
}

#[test]
fn mount_table_rejects_invalid_v0_format() {
    assert_eq!(
        MountEntry::parse("ctx\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\tctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tbad\trbind"),
        Err(MountError::InvalidMode)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\tbind,rbind"),
        Err(MountError::ConflictingBindOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\trbind,rbind"),
        Err(MountError::DuplicateOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\tdev"),
        Err(MountError::InvalidOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro"),
        Err(MountError::WrongFieldCount)
    );
}

#[test]
fn child_agent_authority_accepts_attenuated_owned_child() {
    let parent_identity = AgentUnixIdentity::new(1000, 100, [10, 20, 30]);
    let child_identity = AgentUnixIdentity::new(1000, 100, [10, 30]);
    let parent_policy = PolicyV0::parse(
        "\
allow coder_t tool:fs.read execute
allow coder_t model:debug/echo use
allow coder_t shared:project-a read
",
    );
    assert!(parent_policy.is_ok());
    let Ok(parent_policy) = parent_policy else {
        return;
    };
    let child_policy = PolicyV0::parse(
        "\
allow reviewer_t tool:fs.read execute
allow reviewer_t shared:project-a read
",
    );
    assert!(child_policy.is_ok());
    let Ok(child_policy) = child_policy else {
        return;
    };
    let parent_mounts = MountTable::parse(
        "\
/work\t/work\trw\trbind,nosuid,nodev
/ctx/shared/project-a\t/shared/project-a\tro\trbind,nosuid,nodev,noexec
",
    );
    assert!(parent_mounts.is_ok());
    let Ok(parent_mounts) = parent_mounts else {
        return;
    };
    let child_mounts = MountTable::parse(
        "\
/work\t/work\tro\tbind,nosuid,nodev,noexec
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev,noexec
",
    );
    assert!(child_mounts.is_ok());
    let Ok(child_mounts) = child_mounts else {
        return;
    };

    let request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder session:default run:r123",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
    );
    let authority = ChildAgentAuthority::new(
        "coder",
        &parent_identity,
        "coder_t",
        &parent_policy,
        &parent_mounts,
    );
    assert_eq!(authorize_child_agent(request, authority), Ok(()));
}

#[test]
fn child_agent_authority_rejects_identity_group_policy_and_mount_expansion() {
    let parent_identity = AgentUnixIdentity::new(1000, 100, [10]);
    let child_identity = AgentUnixIdentity::new(1000, 100, [10]);
    let expanded_identity = AgentUnixIdentity::new(1001, 100, [10]);
    let expanded_groups = AgentUnixIdentity::new(1000, 100, [10, 20]);
    let parent_policy = allow_tool_policy("coder_t", "fs.read");
    let child_policy = allow_tool_policy("reviewer_t", "fs.read");
    let expanded_policy = allow_tool_policy("reviewer_t", "shell.exec");
    let parent_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
    assert!(parent_mounts.is_ok());
    let Ok(parent_mounts) = parent_mounts else {
        return;
    };
    let child_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
    assert!(child_mounts.is_ok());
    let Ok(child_mounts) = child_mounts else {
        return;
    };
    let expanded_mounts = MountTable::parse("/work\t/work\trw\tbind,nosuid,nodev,noexec\n");
    assert!(expanded_mounts.is_ok());
    let Ok(expanded_mounts) = expanded_mounts else {
        return;
    };
    let authority = ChildAgentAuthority::new(
        "coder",
        &parent_identity,
        "coder_t",
        &parent_policy,
        &parent_mounts,
    );

    let base = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
    );
    assert_eq!(authorize_child_agent(base, authority), Ok(()));

    let identity_request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(
            &expanded_identity,
            "reviewer_t",
            &child_policy,
            &child_mounts,
        ),
    );
    assert_eq!(
        authorize_child_agent(identity_request, authority),
        Err(ChildAgentDenial::IdentityExpansion)
    );

    let group_request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&expanded_groups, "reviewer_t", &child_policy, &child_mounts),
    );
    assert_eq!(
        authorize_child_agent(group_request, authority),
        Err(ChildAgentDenial::GroupExpansion)
    );

    let policy_request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(
            &child_identity,
            "reviewer_t",
            &expanded_policy,
            &child_mounts,
        ),
    );
    assert_eq!(
        authorize_child_agent(policy_request, authority),
        Err(ChildAgentDenial::PolicyExpansion)
    );

    let mount_request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(
            &child_identity,
            "reviewer_t",
            &child_policy,
            &expanded_mounts,
        ),
    );
    assert_eq!(
        authorize_child_agent(mount_request, authority),
        Err(ChildAgentDenial::MountExpansion)
    );
}

#[test]
fn child_agent_authority_rejects_bad_parent_reference_and_lifecycle() {
    let parent_identity = AgentUnixIdentity::new(1000, 100, [10]);
    let child_identity = AgentUnixIdentity::new(1000, 100, [10]);
    let parent_policy = allow_tool_policy("coder_t", "fs.read");
    let child_policy = allow_tool_policy("reviewer_t", "fs.read");
    let parent_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
    assert!(parent_mounts.is_ok());
    let Ok(parent_mounts) = parent_mounts else {
        return;
    };
    let child_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
    assert!(child_mounts.is_ok());
    let Ok(child_mounts) = child_mounts else {
        return;
    };
    let authority = ChildAgentAuthority::new(
        "coder",
        &parent_identity,
        "coder_t",
        &parent_policy,
        &parent_mounts,
    );

    let mismatch = ChildAgentRequest::new(
        "reviewer",
        "agent:planner",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
    );
    assert_eq!(
        authorize_child_agent(mismatch, authority),
        Err(ChildAgentDenial::ParentMismatch)
    );

    let bad_ref = ChildAgentRequest::new(
        "reviewer",
        "parent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
    );
    assert_eq!(
        authorize_child_agent(bad_ref, authority),
        Err(ChildAgentDenial::InvalidParentRef)
    );

    assert_eq!(
        ChildLifecycle::parse("detached"),
        Err(ChildAgentDenial::UnsupportedLifecycle)
    );
}

#[test]
fn owned_child_cancellation_records_state_and_events_without_deleting_history() {
    let root = unique_test_dir("owned-child-cancel");
    let parent_session = root.join("home").join("1000").join("agent").join("coder");
    let child_session = root.join("home").join("1000").join("agent").join("rev-123");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_text_file(&parent_session.join("events.jsonl"), "");
    create_complete_session_layout(&child_session);
    write_text_file(
        &child_session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"review this\"}\n",
    );
    write_text_file(&child_session.join("events.jsonl"), "");

    let recorded =
        record_owned_child_cancellation("coder", "rev-123", &parent_session, &child_session);
    assert!(recorded.is_ok());
    let Ok(events) = recorded else { return };
    let child_state = fs::read_to_string(child_session.join("state"));
    assert!(child_state.is_ok());
    let Ok(child_state) = child_state else { return };
    assert_eq!(child_state, "cancelled\n");
    let child_messages = fs::read_to_string(child_session.join("messages.jsonl"));
    assert!(child_messages.is_ok());
    let Ok(child_messages) = child_messages else {
        return;
    };
    assert_eq!(
        child_messages,
        "{\"role\":\"user\",\"content\":\"review this\"}\n"
    );

    let parent_events = fs::read_to_string(parent_session.join("events.jsonl"));
    assert!(parent_events.is_ok());
    let Ok(parent_events) = parent_events else {
        return;
    };
    let child_events = fs::read_to_string(child_session.join("events.jsonl"));
    assert!(child_events.is_ok());
    let Ok(child_events) = child_events else {
        return;
    };
    assert_eq!(parent_events, format!("{}\n", events.parent_event()));
    assert_eq!(child_events, format!("{}\n", events.child_event()));
    assert!(inspect_event_stream_jsonl(&events.jsonl()).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn owned_child_cancellation_rejects_bad_names_and_missing_history() {
    let root = unique_test_dir("owned-child-cancel-bad");
    let parent_session = root.join("parent");
    let child_session = root.join("child");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_text_file(&parent_session.join("events.jsonl"), "");
    write_text_file(&child_session.join("events.jsonl"), "");
    write_text_file(&child_session.join("state"), "idle\n");

    assert_eq!(
        owned_child_cancellation_events("bad/parent", "rev-123"),
        Err(OwnedChildCancellationError::InvalidParentName)
    );
    assert_eq!(
        record_owned_child_cancellation("coder", "bad/child", &parent_session, &child_session),
        Err(OwnedChildCancellationError::InvalidChildName)
    );
    assert_eq!(
        record_owned_child_cancellation("coder", "rev-123", &parent_session, &child_session),
        Err(OwnedChildCancellationError::MissingChildHistory)
    );
    assert_eq!(
        OwnedChildCancellationError::MissingChildHistory.errno(),
        "ENOENT"
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn child_context_recorder_creates_handoff_and_result_channel() {
    let root = unique_test_dir("child-context-record");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);

    let handoff = record_child_handoff_to_parent_context(
        &session,
        "rev-2",
        "reviewer",
        "default",
        "Task: review mount ABI\n",
    );
    assert_eq!(handoff, Ok(()));

    let child = session.join("context").join("child").join("rev-2");
    let agent = fs::read_to_string(child.join("agent"));
    assert!(agent.is_ok());
    let Ok(agent) = agent else { return };
    let status = fs::read_to_string(child.join("status"));
    assert!(status.is_ok());
    let Ok(status) = status else { return };
    let handoff = fs::read_to_string(child.join("handoff.md"));
    assert!(handoff.is_ok());
    let Ok(handoff) = handoff else { return };

    assert_eq!(agent, "reviewer\n");
    assert_eq!(status, "pending\n");
    assert_eq!(handoff, "Task: review mount ABI\n");
    assert!(validate_context_pack_source("context/child/rev-2/handoff.md").is_ok());

    let refs =
        r#"{"id":"r1","path":"artifact/report.md","kind":"artifact","summary":"review report"}"#;
    let result = record_child_result_to_parent_context(
        &session,
        "rev-2",
        ChildContextStatus::Done,
        "Summary: ok",
        refs,
    );
    assert_eq!(result, Ok(()));

    let result_md = fs::read_to_string(child.join("result.md"));
    assert!(result_md.is_ok());
    let Ok(result_md) = result_md else { return };
    let refs_jsonl = fs::read_to_string(child.join("refs.jsonl"));
    assert!(refs_jsonl.is_ok());
    let Ok(refs_jsonl) = refs_jsonl else {
        return;
    };
    let status = fs::read_to_string(child.join("status"));
    assert!(status.is_ok());
    let Ok(status) = status else { return };

    assert_eq!(result_md, "Summary: ok\n");
    assert_eq!(status, "done\n");
    assert!(inspect_context_jsonl(ContextJsonlKind::Refs, &refs_jsonl).is_ok());
    assert!(validate_context_pack_source("context/child/rev-2/result.md").is_ok());
    assert!(validate_context_pack_source("context/child/rev-2/refs.jsonl").is_ok());
    assert!(inspect_session_layout(&session).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn child_context_recorder_rejects_bad_names_status_and_refs() {
    let root = unique_test_dir("child-context-record-bad");
    let session = root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "bad/child",
            "reviewer",
            "default",
            "Task: no\n",
        ),
        Err(ChildContextRecordError::InvalidChildName)
    );
    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-2",
            "reviewer",
            "default",
            "Task: no\n",
        ),
        Ok(())
    );
    assert_eq!(
        record_child_result_to_parent_context(
            &session,
            "rev-2",
            ChildContextStatus::Pending,
            "not terminal",
            "",
        ),
        Err(ChildContextRecordError::InvalidStatus)
    );
    assert_eq!(
        record_child_result_to_parent_context(
            &session,
            "rev-2",
            ChildContextStatus::Done,
            "done",
            "{\"path\":\"../secret\"}\n",
        ),
        Err(ChildContextRecordError::InvalidRefs)
    );
    assert_eq!(ChildContextRecordError::InvalidRefs.errno(), "EINVAL");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_layout_inspector_accepts_transparent_context_tree() {
    let root = unique_test_dir("session-layout-ok");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&root);

    let report = inspect_session_layout(&root);
    assert!(report.is_ok());
    assert!(report.issues().is_empty());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_layout_inspector_reports_missing_and_wrong_types() {
    let root = unique_test_dir("session-layout-bad");
    let context = root.join("context");
    let child = context.join("child").join("rev-1");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(root.join("messages.jsonl")).is_ok());
    assert!(fs::create_dir_all(&child).is_ok());
    assert!(fs::write(child.join("agent"), "reviewer\n").is_ok());
    assert!(fs::create_dir_all(context.join("pack.md")).is_ok());

    let report = inspect_session_layout(&root);
    assert!(!report.is_ok());
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::NotFile("messages.jsonl".to_owned())));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::MissingFile("events.jsonl".to_owned())));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::NotFile("context/pack.md".to_owned())));
    assert!(report.issues().contains(&SessionLayoutIssue::MissingFile(
        "context/child/rev-1/result.md".to_owned()
    )));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::MissingDirectory(
            "context/child/rev-1/artifact".to_owned()
        )));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_controls_accept_fixed_v1_values() {
    assert!(inspect_session_control(SessionControlKind::State, "active\n").is_ok());
    assert!(inspect_session_control(SessionControlKind::State, "cancelled\n").is_ok());
    assert!(inspect_session_control(SessionControlKind::Cwd, "/work/project\n").is_ok());
    assert!(inspect_session_control(
        SessionControlKind::MetaJson,
        "{\"client\":\"ctx\",\"model\":\"debug/echo\",\"scope\":\"shared\"}\n"
    )
    .is_ok());
    assert!(inspect_session_control(SessionControlKind::MetaJson, "{}\n").is_ok());
}

#[test]
fn session_controls_reject_invalid_state_cwd_and_meta() {
    assert_eq!(
        inspect_session_control(SessionControlKind::State, "running\n").issues(),
        &[SessionControlIssue::InvalidValue {
            line: 1,
            value: "running".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::Cwd, "../work\n").issues(),
        &[SessionControlIssue::InvalidValue {
            line: 1,
            value: "../work".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::Cwd, "/work/../secret\n").issues(),
        &[SessionControlIssue::InvalidValue {
            line: 1,
            value: "/work/../secret".to_owned()
        }]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "{").issues(),
        &[SessionControlIssue::InvalidJson]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "[]\n").issues(),
        &[SessionControlIssue::NotObject]
    );
    assert_eq!(
        inspect_session_control(SessionControlKind::MetaJson, "{\"scope\":\"global\"}\n").issues(),
        &[SessionControlIssue::InvalidValue {
            line: 1,
            value: "global".to_owned()
        }]
    );
}

#[test]
fn session_layout_inspector_rejects_invalid_control_values() {
    let root = unique_test_dir("session-layout-control-bad");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&root);
    write_text_file(&root.join("state"), "running\n");
    write_text_file(&root.join("cwd"), "/work/../secret\n");
    write_text_file(&root.join("meta.json"), "{\"model\":\"bad/model/extra\"}\n");

    let report = inspect_session_layout(&root);
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::InvalidFileValue {
            path: "state".to_owned(),
            value: "running".to_owned()
        }));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::InvalidFileValue {
            path: "cwd".to_owned(),
            value: "/work/../secret".to_owned()
        }));
    assert!(report
        .issues()
        .contains(&SessionLayoutIssue::InvalidFileValue {
            path: "meta.json".to_owned(),
            value: "bad/model/extra".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_index_accepts_fixed_formats() {
    assert!(inspect_session_index(SessionIndexKind::List, "default\nreview-1\n").is_ok());
    assert!(inspect_session_index(SessionIndexKind::Current, "default\n").is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByCwd, "worktree-1").is_ok());
    assert!(inspect_session_index(SessionIndexKind::List, "").is_ok());
}

#[test]
fn session_index_rejects_invalid_names_and_multi_value_files() {
    let list = inspect_session_index(SessionIndexKind::List, "default\nbad/name\n\n spaced\n");
    assert_eq!(
        list.issues(),
        &[
            SessionIndexIssue::InvalidSessionName {
                line: 2,
                value: "bad/name".to_owned()
            },
            SessionIndexIssue::EmptyValue { line: 3 },
            SessionIndexIssue::InvalidSessionName {
                line: 4,
                value: "spaced".to_owned()
            }
        ]
    );

    let current = inspect_session_index(SessionIndexKind::Current, "default\nother\n");
    assert_eq!(
        current.issues(),
        &[SessionIndexIssue::MultipleValues { line: 2 }]
    );

    let empty = inspect_session_index(SessionIndexKind::ByCwd, "");
    assert_eq!(empty.issues(), &[SessionIndexIssue::EmptyValue { line: 1 }]);
}

#[test]
fn session_index_update_sets_current_and_deduplicated_list() {
    let root = unique_test_dir("session-index-update");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    assert!(fs::create_dir_all(session_root.join("review-1")).is_ok());
    write_text_file(
        &session_root.join("index").join("list"),
        "default\nreview-1\n",
    );
    write_text_file(&session_root.join("index").join("current"), "default\n");

    let updated = update_session_index(&session_root, "review-1", Some("cwd-hash-1"));
    assert_eq!(updated, Ok(()));
    let list = fs::read_to_string(session_root.join("index").join("list"));
    assert!(list.is_ok());
    let Ok(list) = list else { return };
    let current = fs::read_to_string(session_root.join("index").join("current"));
    assert!(current.is_ok());
    let Ok(current) = current else { return };
    let by_cwd = fs::read_to_string(session_root.join("index").join("by-cwd").join("cwd-hash-1"));
    assert!(by_cwd.is_ok());
    let Ok(by_cwd) = by_cwd else { return };

    assert_eq!(list, "review-1\ndefault\n");
    assert_eq!(current, "review-1\n");
    assert_eq!(by_cwd, "review-1\n");
    assert!(inspect_session_index(SessionIndexKind::List, &list).is_ok());
    assert!(inspect_session_index(SessionIndexKind::Current, &current).is_ok());
    assert!(inspect_session_index(SessionIndexKind::ByCwd, &by_cwd).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_index_update_rejects_missing_and_invalid_index_state() {
    let root = unique_test_dir("session-index-update-bad");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
    assert!(fs::create_dir_all(session_root.join("default")).is_ok());
    write_text_file(&session_root.join("index").join("list"), "bad/name\n");
    write_text_file(&session_root.join("index").join("current"), "default\n");

    assert_eq!(
        update_session_index(&session_root, "bad/name", None),
        Err(SessionIndexUpdateError::InvalidSessionName)
    );
    assert_eq!(
        update_session_index(&session_root, "missing", None),
        Err(SessionIndexUpdateError::MissingSession)
    );
    assert_eq!(
        update_session_index(&session_root, "default", Some("bad/key")),
        Err(SessionIndexUpdateError::InvalidByCwdKey)
    );
    assert_eq!(
        update_session_index(&session_root, "default", None),
        Err(SessionIndexUpdateError::InvalidIndex)
    );
    assert_eq!(SessionIndexUpdateError::InvalidIndex.errno(), "EINVAL");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn context_pack_sources_are_session_relative_and_inspectable() {
    let report = inspect_context_pack_json(
        r#"{
  "session": "default",
  "agent": "coder",
  "items": [
{"kind": "summary", "source": "context/summary.md"},
{"kind": "messages", "source": "messages.jsonl"},
{"kind": "child_result", "source": "context/child/rev-1/result.md"},
{"kind": "child_refs", "source": "context/child/rev-1/refs.jsonl"},
{"kind": "artifact", "source": "context/child/rev-1/artifact/report.md"},
{"kind": "pinned", "source": "context/pinned/system.md"}
  ]
}"#,
    );
    assert!(report.is_ok());
    assert!(validate_context_pack_source("context/facts.jsonl").is_ok());
}

#[test]
fn context_pack_sources_reject_escapes_and_child_history() {
    assert_eq!(
        validate_context_pack_source("/ctx/shared/im-a/agent/bot/session/group-1/messages.jsonl"),
        Err(ContextPackSourceError::Absolute)
    );
    assert_eq!(
        validate_context_pack_source("../other/messages.jsonl"),
        Err(ContextPackSourceError::ParentComponent)
    );
    assert_eq!(
        validate_context_pack_source("session/other/messages.jsonl"),
        Err(ContextPackSourceError::UnsupportedSessionPath)
    );
    assert_eq!(
        validate_context_pack_source("context/child/rev-1/messages.jsonl"),
        Err(ContextPackSourceError::UnsupportedChildPath)
    );

    let report = inspect_context_pack_json(
        r#"{
  "items": [
{"kind": "ok", "source": "context/summary.md"},
{"kind": "absolute", "source": "/ctx/shared/im-b/agent/bot/session/channel-2/messages.jsonl"},
{"kind": "child_full_history", "source": "context/child/rev-1/messages.jsonl"},
{"kind": "missing"},
{"kind": "not_string", "source": 42}
  ]
}"#,
    );
    assert!(!report.is_ok());
    assert_eq!(
        report.issues(),
        [
            ContextPackIssue::InvalidSource {
                item: 1,
                source: "/ctx/shared/im-b/agent/bot/session/channel-2/messages.jsonl".to_owned(),
                reason: ContextPackSourceError::Absolute
            },
            ContextPackIssue::InvalidSource {
                item: 2,
                source: "context/child/rev-1/messages.jsonl".to_owned(),
                reason: ContextPackSourceError::UnsupportedChildPath
            },
            ContextPackIssue::MissingSource(3),
            ContextPackIssue::SourceNotString(4)
        ]
    );
}

#[test]
fn context_pack_rebuild_writes_inspectable_sources_without_child_history() {
    let root = unique_test_dir("context-pack-rebuild");
    let session = root.join("default");
    let context = session.join("context");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"system\",\"content\":\"base rules\"}\n{\"role\":\"user\",\"content\":\"fix tests\"}\n{\"role\":\"assistant\",\"content\":\"working\"}\n",
    );
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(
        &context.join("pinned").join("system.md"),
        "Pinned system text\n",
    );
    write_text_file(&context.join("summary.md"), "Short summary\n");
    write_text_file(
        &context.join("facts.jsonl"),
        "{\"id\":\"f1\",\"text\":\"Root ABI is frozen.\",\"source\":\"messages:1-2\"}\n",
    );
    write_text_file(
        &context.join("decisions.jsonl"),
        "{\"id\":\"d1\",\"decision\":\"Do not add provider root.\",\"source\":\"messages:3\"}\n",
    );
    write_text_file(&context.join("todo.md"), "Keep FUSE small\n");
    write_text_file(
        &context.join("refs.jsonl"),
        "{\"id\":\"r1\",\"path\":\"docs/spec/16-context.md\",\"kind\":\"file\",\"summary\":\"context spec\"}\n",
    );
    write_text_file(
        &context.join("child").join("rev-1").join("result.md"),
        "Child says ok\n",
    );
    write_text_file(
        &context.join("child").join("rev-1").join("refs.jsonl"),
        "{\"id\":\"cr1\",\"path\":\"artifact/report.md\",\"kind\":\"artifact\",\"summary\":\"child report\"}\n",
    );
    write_text_file(
        &context.join("child").join("rev-1").join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"must not be packed\"}\n",
    );

    let built = rebuild_context_pack(&session, Some("coder"), 2);
    assert!(built.is_ok());
    let Ok(built) = built else { return };

    let pack_json = fs::read_to_string(context.join("pack.json"));
    assert!(pack_json.is_ok());
    let Ok(pack_json) = pack_json else { return };
    let pack_md = fs::read_to_string(context.join("pack.md"));
    assert!(pack_md.is_ok());
    let Ok(pack_md) = pack_md else { return };

    assert_eq!(built.pack_json(), pack_json);
    assert_eq!(built.pack_md(), pack_md);
    assert!(inspect_context_pack_json(&pack_json).is_ok());
    assert!(pack_json.contains("\"source\":\"context/pinned/system.md\""));
    assert!(pack_json.contains("\"source\":\"messages.jsonl\""));
    assert!(pack_json.contains("\"range\":\"tail:2\""));
    assert!(pack_json.contains("\"source\":\"context/child/rev-1/result.md\""));
    assert!(pack_json.contains("\"source\":\"context/child/rev-1/refs.jsonl\""));
    assert!(!pack_json.contains("context/child/rev-1/messages.jsonl"));
    assert!(pack_md.contains("Pinned system text"));
    assert!(pack_md.contains("Child says ok"));
    assert!(pack_md.contains("\"role\":\"assistant\""));
    assert!(!pack_md.contains("must not be packed"));
    assert!(built.items().iter().all(|item| {
        validate_context_pack_source(item.source()).is_ok()
            && item.source() != "context/child/rev-1/messages.jsonl"
    }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn context_pack_rebuild_respects_budget_and_validates_inputs() {
    let root = unique_test_dir("context-pack-rebuild-budget");
    let session = root.join("default");
    let context = session.join("context");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"one two three four five six\"}\n",
    );
    write_text_file(&context.join("budget"), "2\n");
    write_text_file(&context.join("summary.md"), "one two\n");
    write_text_file(&context.join("facts.jsonl"), "");
    write_text_file(&context.join("decisions.jsonl"), "");
    write_text_file(&context.join("todo.md"), "");
    write_text_file(&context.join("refs.jsonl"), "");
    write_text_file(&context.join("child").join("rev-1").join("result.md"), "");
    write_text_file(&context.join("child").join("rev-1").join("refs.jsonl"), "");

    let built = rebuild_context_pack(&session, Some("coder"), 5);
    assert!(built.is_ok());
    let Ok(built) = built else { return };
    assert_eq!(built.items().len(), 1);
    assert_eq!(
        built
            .items()
            .first()
            .map(super::ContextPackBuiltItem::source),
        Some("context/summary.md")
    );
    assert!(!built.pack_json().contains("messages.jsonl"));

    write_text_file(&context.join("budget"), " 2\n");
    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::InvalidBudget)
    );
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"native_thread\"}\n",
    );
    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::InvalidMessages)
    );
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"ok\"}\n",
    );
    assert_eq!(
        rebuild_context_pack(&session, Some("bad/agent"), 5),
        Err(ContextPackBuildError::InvalidAgentName)
    );
    assert!(fs::create_dir_all(context.join("child").join(".bad")).is_ok());
    assert_eq!(
        rebuild_context_pack(&session, Some("coder"), 5),
        Err(ContextPackBuildError::InvalidChildName)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn context_pack_rejects_invalid_json_shape() {
    assert_eq!(
        inspect_context_pack_json("{").issues(),
        &[ContextPackIssue::InvalidJson]
    );
    assert_eq!(
        inspect_context_pack_json(r#"{"items": {"source": "messages.jsonl"}}"#).issues(),
        &[ContextPackIssue::ItemsNotArray]
    );
    assert_eq!(
        inspect_context_pack_json(r#"{"items": ["messages.jsonl"]}"#).issues(),
        &[ContextPackIssue::ItemNotObject(0)]
    );
}

#[test]
fn message_stream_accepts_canonical_role_content_frames() {
    let report = inspect_message_stream_jsonl(
        r#"{"role":"system","content":"You are concise."}
{"role":"user","content":[{"type":"text","text":"hello"}]}
{"role":"assistant","content":[{"type":"text","text":"hi"}]}
{"role":"tool","content":[{"type":"tool_result","tool_call_id":"call-1","content":"ok"}]}
"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn message_stream_rejects_native_state_and_bad_shape() {
    let report = inspect_message_stream_jsonl(
        r#"not-json
[]
{"content":"missing role"}
{"role":"developer","content":"private role"}
{"role":"assistant","response_id":"resp-1","content":"hi"}
{"role":"assistant","content":[{"type":"provider_blob","text":"x"}]}
{"role":"assistant"}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            MessageStreamIssue::InvalidJson(1),
            MessageStreamIssue::MessageNotObject(2),
            MessageStreamIssue::MissingRole(3),
            MessageStreamIssue::InvalidRole {
                line: 4,
                role: "developer".to_owned()
            },
            MessageStreamIssue::ProviderNativeField {
                line: 5,
                field: "response_id".to_owned()
            },
            MessageStreamIssue::InvalidContent(6),
            MessageStreamIssue::MissingContent(7)
        ]
    );
}

#[test]
fn context_jsonl_accepts_spec_record_shapes() {
    assert!(inspect_context_jsonl(
        ContextJsonlKind::Facts,
        r#"{"id":"f1","text":"CortexFS root is small.","source":"messages:12-18"}
"#
    )
    .is_ok());
    assert!(inspect_context_jsonl(
        ContextJsonlKind::Decisions,
        r#"{"id":"d1","decision":"Child agents are owned.","source":"user:latest"}
"#
    )
    .is_ok());
    assert!(inspect_context_jsonl(
        ContextJsonlKind::Refs,
        r#"{"id":"r1","path":"/work/DESIGN.md","kind":"file","summary":"design"}
{"id":"r2","path":"context/swap/chunk/sha256-abc","kind":"swap","summary":"old design"}
"#
    )
    .is_ok());
    assert!(
        inspect_context_jsonl(
            ContextJsonlKind::SwapIndex,
            r#"{"id":"sha256-abc","kind":"message_range","source":"messages.jsonl","summary":"initial design","tokens":18000}
{"id":"sha256-def","kind":"tool_output","source":"events.jsonl","summary":"test output","tokens":45000}
"#
        )
        .is_ok()
    );
    assert!(
        inspect_context_jsonl(
            ContextJsonlKind::DedupIndex,
            r#"{"hash":"sha256-abc","refs":["messages:1-40","swap:old-design"],"bytes":12000,"tokens":3000}
"#
        )
        .is_ok()
    );
}

#[test]
fn context_jsonl_rejects_invalid_records() {
    let facts = inspect_context_jsonl(
        ContextJsonlKind::Facts,
        "not-json\n[]\n{\"id\":\"bad/id\",\"text\":\"ok\"}\n",
    );
    assert_eq!(
        facts.issues(),
        [
            ContextJsonlIssue::InvalidJson(1),
            ContextJsonlIssue::RecordNotObject(2),
            ContextJsonlIssue::InvalidField {
                line: 3,
                field: "id".to_owned(),
                value: "bad/id".to_owned()
            },
            ContextJsonlIssue::MissingStringField {
                line: 3,
                field: "source".to_owned()
            }
        ]
    );

    let refs = inspect_context_jsonl(
        ContextJsonlKind::Refs,
        r#"{"id":"r1","path":"../secret","kind":"provider_thread","summary":"bad"}
"#,
    );
    assert_eq!(
        refs.issues(),
        [
            ContextJsonlIssue::InvalidField {
                line: 1,
                field: "path".to_owned(),
                value: "../secret".to_owned()
            },
            ContextJsonlIssue::InvalidField {
                line: 1,
                field: "kind".to_owned(),
                value: "provider_thread".to_owned()
            }
        ]
    );

    let dedup = inspect_context_jsonl(
        ContextJsonlKind::DedupIndex,
        r#"{"hash":"md5-old","refs":[],"bytes":"120","tokens":3000}
"#,
    );
    assert_eq!(
        dedup.issues(),
        [
            ContextJsonlIssue::InvalidField {
                line: 1,
                field: "hash".to_owned(),
                value: "md5-old".to_owned()
            },
            ContextJsonlIssue::MissingStringArrayField {
                line: 1,
                field: "refs".to_owned()
            },
            ContextJsonlIssue::MissingNumberField {
                line: 1,
                field: "bytes".to_owned()
            }
        ]
    );
}

#[test]
fn event_stream_accepts_canonical_model_jsonl() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"start","run":"r1","model":"debug/echo"}
{"type":"delta","run":"r1","text":"hello"}
{"type":"message","run":"r1","role":"assistant","content":[{"type":"text","text":"hello"}]}
{"type":"tool_call","run":"r1","id":"call-1","name":"fs.read","arguments":{"path":"README.md"}}
{"type":"usage","run":"r1","input_tokens":10,"output_tokens":1}
{"type":"done","run":"r1","status":"ok"}
"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn event_stream_accepts_stable_error_frames() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"error","run":"r1","code":"EACCES","message":"permission denied"}
{"type":"done","run":"r1","status":"error"}
"#,
    );
    assert!(report.is_ok());
}

#[test]
fn event_stream_accepts_child_lifecycle_frames() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"agent.child.cancel","parent":"coder","child":"rev-123","reason":"parent_dead"}
{"type":"agent.stop","agent":"rev-123","status":"cancelled"}
"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn event_stream_rejects_provider_native_state_and_unknown_events() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"start","run":"r1","model":"debug/echo","response_id":"resp_123"}
{"type":"native_thread","run":"r1","thread_id":"thread_123"}
{"type":"message","run":"r1","content":[{"type":"text","text":"x","provider_response_id":"abc"}]}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            EventStreamIssue::ProviderNativeField {
                line: 1,
                field: "response_id".to_owned()
            },
            EventStreamIssue::ProviderNativeField {
                line: 2,
                field: "thread_id".to_owned()
            },
            EventStreamIssue::UnknownType {
                line: 2,
                event_type: "native_thread".to_owned()
            },
            EventStreamIssue::ProviderNativeField {
                line: 3,
                field: "provider_response_id".to_owned()
            }
        ]
    );
}

#[test]
fn event_stream_rejects_invalid_shape_and_specialized_frames() {
    let report = inspect_event_stream_jsonl(
        r#"not-json
[]
{"run":"r1"}
{"type":"delta","text":"missing run"}
{"type":"error","run":"r1","code":"PROVIDER_DENIED"}
{"type":"done","run":"r1","status":"maybe"}
{"type":"usage","run":"r1","input_tokens":"10","output_tokens":1}
{"type":"tool_call","run":"r1","id":"bad/id","name":"fs.read"}
{"type":"agent.child.cancel","parent":"bad/parent","child":"rev-1","reason":"manual"}
{"type":"agent.stop","agent":"rev-1","status":"dead"}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            EventStreamIssue::InvalidJson(1),
            EventStreamIssue::EventNotObject(2),
            EventStreamIssue::MissingType(3),
            EventStreamIssue::MissingRun(4),
            EventStreamIssue::InvalidErrorCode(5),
            EventStreamIssue::InvalidDoneStatus(6),
            EventStreamIssue::InvalidUsage(7),
            EventStreamIssue::InvalidToolCall(8),
            EventStreamIssue::InvalidAgentLifecycle(9),
            EventStreamIssue::InvalidAgentLifecycle(10)
        ]
    );
}

#[test]
fn shared_queue_layout_inspector_checks_recommended_dirs() {
    let root = unique_test_dir("shared-queue-layout");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        assert!(fs::create_dir_all(root.join(dir)).is_ok());
    }
    let report = inspect_shared_queue_layout(&root);
    assert!(report.is_ok());

    assert!(fs::remove_dir_all(root.join("failed")).is_ok());
    assert!(fs::remove_dir_all(root.join("done")).is_ok());
    assert!(fs::write(root.join("done"), "not a dir\n").is_ok());
    let report = inspect_shared_queue_layout(&root);
    assert!(!report.is_ok());
    assert!(report
        .issues()
        .contains(&SharedQueueLayoutIssue::MissingDirectory(
            "failed".to_owned()
        )));
    assert!(report
        .issues()
        .contains(&SharedQueueLayoutIssue::NotDirectory("done".to_owned())));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_queue_claim_uses_atomic_claim_directories() {
    let root = unique_test_dir("shared-queue-claim");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-2.req.json"), "two\n");
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    write_text_file(&root.join("pending").join(".ignored"), "bad\n");
    assert!(fs::create_dir_all(root.join("pending").join("not-file")).is_ok());

    let first = claim_next_shared_queue_job(&root, "worker-a");
    assert!(first.is_ok());
    let Ok(Some(first)) = first else { return };
    assert_eq!(first.job_name(), "job-1.req.json");
    let claimed_content = fs::read_to_string(first.claimed_path());
    assert!(matches!(claimed_content, Ok(ref content) if content == "one\n"));
    let lease_worker = fs::read_to_string(first.lease_path().join("worker"));
    assert!(matches!(lease_worker, Ok(ref content) if content == "worker-a\n"));
    assert!(!root.join("pending").join("job-1.req.json").exists());

    let second = claim_next_shared_queue_job(&root, "worker-b");
    assert!(second.is_ok());
    let Ok(Some(second)) = second else { return };
    assert_eq!(second.job_name(), "job-2.req.json");

    let none = claim_next_shared_queue_job(&root, "worker-c");
    assert_eq!(none, Ok(None));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_queue_claim_skips_existing_claim_lock() {
    let root = unique_test_dir("shared-queue-claim-lock");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
    write_text_file(&root.join("pending").join("job-2.req.json"), "two\n");
    assert!(fs::create_dir_all(root.join("claimed").join("job-1.req.json")).is_ok());

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    assert!(claimed.is_ok());
    let Ok(Some(claimed)) = claimed else { return };
    assert_eq!(claimed.job_name(), "job-2.req.json");
    assert!(root.join("pending").join("job-1.req.json").exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_queue_recovery_requeues_claimed_job_with_lease() {
    let root = unique_test_dir("shared-queue-recover");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    assert!(claimed.is_ok());
    let Ok(Some(claimed)) = claimed else { return };
    assert!(claimed.claimed_path().is_file());
    assert!(claimed.lease_path().join("worker").is_file());

    let recovered = recover_shared_queue_job(&root, "job-1.req.json");
    assert_eq!(recovered, Ok(root.join("pending").join("job-1.req.json")));
    let recovered_content = fs::read_to_string(root.join("pending").join("job-1.req.json"));
    assert!(matches!(recovered_content, Ok(ref content) if content == "one\n"));
    assert!(!root.join("claimed").join("job-1.req.json").exists());
    assert!(!root.join("lease").join("job-1.req.json").exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_queue_recovery_requires_existing_claim_and_lease() {
    let root = unique_test_dir("shared-queue-recover-missing");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_shared_queue_layout(&root);
    assert_eq!(
        recover_shared_queue_job(&root, "job-1.req.json"),
        Err(SharedQueueRecoverError::MissingClaim)
    );

    let claim_dir = root.join("claimed").join("job-1.req.json");
    assert!(fs::create_dir_all(&claim_dir).is_ok());
    write_text_file(&claim_dir.join("job-1.req.json"), "one\n");
    assert_eq!(
        recover_shared_queue_job(&root, "job-1.req.json"),
        Err(SharedQueueRecoverError::MissingLease)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_queue_finish_writes_readable_done_result_and_cleans_lease() {
    let root = unique_test_dir("shared-queue-finish-done");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    assert!(claimed.is_ok());
    let Ok(Some(claimed)) = claimed else { return };
    let result_path =
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n");
    assert_eq!(
        result_path,
        Ok(root.join("done").join("job-1.req.json.result"))
    );
    let result = fs::read_to_string(root.join("done").join("job-1.req.json.result"));
    assert!(matches!(result, Ok(ref content) if content == "ok\n"));
    let request = fs::read_to_string(root.join("done").join("job-1.req.json"));
    assert!(matches!(request, Ok(ref content) if content == "one\n"));
    assert!(!root.join("claimed").join("job-1.req.json").exists());
    assert!(!root.join("lease").join("job-1.req.json").exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_queue_finish_writes_readable_failed_result() {
    let root = unique_test_dir("shared-queue-finish-failed");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    assert!(claimed.is_ok());
    let Ok(Some(claimed)) = claimed else { return };
    let result_path = finish_shared_queue_job(
        &root,
        claimed.job_name(),
        SharedQueueOutcome::Failed,
        b"err\n",
    );
    assert_eq!(
        result_path,
        Ok(root.join("failed").join("job-1.req.json.result"))
    );
    let result = fs::read_to_string(root.join("failed").join("job-1.req.json.result"));
    assert!(matches!(result, Ok(ref content) if content == "err\n"));
    let request = fs::read_to_string(root.join("failed").join("job-1.req.json"));
    assert!(matches!(request, Ok(ref content) if content == "one\n"));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_access_authority_requires_mount_linux_permission_and_policy() {
    let root = unique_test_dir("shared-authority-ok");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o400);

    let metadata = fs::metadata(&file);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_source_target(
        "/ctx/shared/project-a",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);
    let authority = SharedAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Read, authority),
        Ok(())
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_access_authority_denies_write_on_read_only_mount() {
    let root = unique_test_dir("shared-authority-ro");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o600);

    let metadata = fs::metadata(&file);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts =
        mount_table_for_source_target("/ctx/shared/project-a", &shared, "ro", "bind,nosuid,nodev");
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Write);
    let authority = SharedAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Write, authority),
        Err(SharedAccessDenial::ReadOnlyMount)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_access_authority_denies_missing_policy_and_wrong_space() {
    let root = unique_test_dir("shared-authority-policy");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o400);

    let metadata = fs::metadata(&file);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts =
        mount_table_for_source_target("/ctx/shared/project-a", &shared, "ro", "bind,nosuid,nodev");
    let wrong_mounts =
        mount_table_for_source_target("/ctx/shared/project-b", &shared, "ro", "bind,nosuid,nodev");
    let empty_policy = PolicyV0::parse("");
    assert!(empty_policy.is_ok());
    let Ok(empty_policy) = empty_policy else {
        return;
    };
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);

    assert_eq!(
        authorize_shared_access(
            "project-a",
            &file,
            SharedAccess::Read,
            SharedAccessAuthority::new(&identity, &mounts, "coder_t", &empty_policy),
        ),
        Err(SharedAccessDenial::Policy)
    );
    assert_eq!(
        authorize_shared_access(
            "project-a",
            &file,
            SharedAccess::Read,
            SharedAccessAuthority::new(&identity, &wrong_mounts, "coder_t", &policy),
        ),
        Err(SharedAccessDenial::WrongSharedPath)
    );
    assert_eq!(
        authorize_shared_access(
            "project-a",
            &file,
            SharedAccess::Read,
            SharedAccessAuthority::new(&identity, &MountTable::default(), "coder_t", &policy,),
        ),
        Err(SharedAccessDenial::NotMounted)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn shared_access_authority_checks_linux_mode_bits() {
    let root = unique_test_dir("shared-authority-linux");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o400);

    let metadata = fs::metadata(&file);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let other_identity = AgentUnixIdentity::new(
        metadata.uid().saturating_add(1),
        metadata.gid().saturating_add(1),
        [],
    );
    let mounts =
        mount_table_for_source_target("/ctx/shared/project-a", &shared, "ro", "bind,nosuid,nodev");
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);
    let authority = SharedAccessAuthority::new(&other_identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Read, authority),
        Err(SharedAccessDenial::LinuxPermission)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_access_authority_allows_explicit_im_channel_session() {
    let root = unique_test_dir("session-authority-im-ok");
    let shared = root.join("im-qq-dev");
    let messages = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-456")
        .join("messages.jsonl");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_fixture_file(&messages, 0o600);

    let metadata = fs::metadata(&messages);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_source_target(
        "/ctx/shared/im-qq-dev",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = policy_with_rules([
        "allow bot_t shared:im-qq-dev read",
        "allow bot_t session:group-456 read",
    ]);
    let authority = SessionAccessAuthority::new(&identity, &mounts, "bot_t", &policy);

    assert_eq!(
        authorize_session_access(&messages, SessionAccess::Read, authority),
        Ok(())
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_access_authority_denies_cross_channel_without_session_policy() {
    let root = unique_test_dir("session-authority-im-deny");
    let shared = root.join("im-qq-dev");
    let allowed = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-456")
        .join("messages.jsonl");
    let other = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-999")
        .join("messages.jsonl");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_fixture_file(&allowed, 0o600);
    write_fixture_file(&other, 0o600);

    let metadata = fs::metadata(&allowed);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_source_target(
        "/ctx/shared/im-qq-dev",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = policy_with_rules([
        "allow bot_t shared:im-qq-dev read",
        "allow bot_t session:group-456 read",
    ]);
    let authority = SessionAccessAuthority::new(&identity, &mounts, "bot_t", &policy);

    assert_eq!(
        authorize_session_access(&allowed, SessionAccess::Read, authority),
        Ok(())
    );
    assert_eq!(
        authorize_session_access(&other, SessionAccess::Read, authority),
        Err(SessionAccessDenial::SessionPolicy)
    );
    assert_eq!(SessionAccessDenial::SessionPolicy.errno(), "EACCES");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_access_authority_requires_shared_policy_and_mount_write_mode() {
    let root = unique_test_dir("session-authority-shared-policy");
    let shared = root.join("im-slack-company");
    let messages = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("channel-789")
        .join("messages.jsonl");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_fixture_file(&messages, 0o600);

    let metadata = fs::metadata(&messages);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let ro_mounts = mount_table_for_source_target(
        "/ctx/shared/im-slack-company",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let writable_mounts = mount_table_for_source_target(
        "/ctx/shared/im-slack-company",
        &shared,
        "rw",
        "bind,nosuid,nodev",
    );
    let session_only = policy_with_rules(["allow bot_t session:channel-789 read"]);
    let read_policy = policy_with_rules([
        "allow bot_t shared:im-slack-company read",
        "allow bot_t session:channel-789 write",
    ]);

    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Read,
            SessionAccessAuthority::new(&identity, &ro_mounts, "bot_t", &session_only),
        ),
        Err(SessionAccessDenial::SharedPolicy)
    );
    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Write,
            SessionAccessAuthority::new(&identity, &ro_mounts, "bot_t", &read_policy),
        ),
        Err(SessionAccessDenial::ReadOnlyMount)
    );
    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Write,
            SessionAccessAuthority::new(&identity, &writable_mounts, "bot_t", &read_policy),
        ),
        Err(SessionAccessDenial::SharedPolicy)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_access_authority_enforces_private_home_uid() {
    let root = unique_test_dir("session-authority-private-uid");
    let home = root.join("home-1000");
    let messages = home
        .join("agent")
        .join("coder")
        .join("session")
        .join("default")
        .join("messages.jsonl");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_fixture_file(&messages, 0o644);

    let metadata = fs::metadata(&messages);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let owner_identity = AgentUnixIdentity::new(1000, metadata.gid(), []);
    let other_identity = AgentUnixIdentity::new(1001, metadata.gid(), []);
    let mounts =
        mount_table_for_source_target("/ctx/home/1000", &home, "ro", "bind,nosuid,nodev,noexec");
    let policy = policy_with_rules(["allow coder_t session:default read"]);

    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Read,
            SessionAccessAuthority::new(&owner_identity, &mounts, "coder_t", &policy),
        ),
        Ok(())
    );
    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Read,
            SessionAccessAuthority::new(&other_identity, &mounts, "coder_t", &policy),
        ),
        Err(SessionAccessDenial::LinuxPermission)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn session_access_authority_rejects_unmounted_and_non_session_paths() {
    let root = unique_test_dir("session-authority-path-shape");
    let shared = root.join("project-a");
    let file = shared.join("data").join("note.txt");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    write_fixture_file(&file, 0o644);

    let metadata = fs::metadata(&file);
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_source_target(
        "/ctx/shared/project-a",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = policy_with_rules([
        "allow coder_t shared:project-a read",
        "allow coder_t session:default read",
    ]);

    assert_eq!(
        authorize_session_access(
            &file,
            SessionAccess::Read,
            SessionAccessAuthority::new(&identity, &mounts, "coder_t", &policy),
        ),
        Err(SessionAccessDenial::InvalidSessionPath)
    );
    assert_eq!(
        authorize_session_access(
            &file,
            SessionAccess::Read,
            SessionAccessAuthority::new(&identity, &MountTable::default(), "coder_t", &policy),
        ),
        Err(SessionAccessDenial::NotMounted)
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn ctx_path_parses_without_implicit_current_directory() {
    let path = ToolPath::parse(":/ctx/tool::/ctx/home/1000/tool:");
    assert_eq!(
        path.dirs(),
        [
            PathBuf::from("/ctx/tool"),
            PathBuf::from("/ctx/home/1000/tool")
        ]
    );
}

#[test]
fn tool_lookup_uses_first_executable_hit() {
    let root = unique_test_dir("tool-lookup");
    let global = root.join("global-tool");
    let user = root.join("user-tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(&global).is_ok());
    assert!(fs::create_dir_all(&user).is_ok());

    write_fixture_file(&global.join("fs.read"), 0o644);
    write_fixture_file(&global.join("fs.write"), 0o755);
    write_fixture_file(&user.join("fs.read"), 0o755);
    assert!(fs::create_dir_all(user.join("fs.read.d")).is_ok());

    let path = ToolPath::new([global.clone(), user.clone()]);
    let found = path.find("fs.read");
    assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == user.join("fs.read")));
    assert!(matches!(found, Ok(Some(ref hit)) if hit.control_dir() == user.join("fs.read.d")));

    write_fixture_file(&global.join("fs.read"), 0o755);
    let found = path.find("fs.read");
    assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == global.join("fs.read")));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_listing_ignores_non_executable_and_control_entries() {
    let root = unique_test_dir("tool-list");
    let tools = root.join("tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);
    write_fixture_file(&tools.join("not.exec"), 0o644);
    write_fixture_file(&tools.join("bad.sock"), 0o755);

    let hits = ToolPath::new([tools.clone()]).list();
    assert!(hits.is_ok());
    let Ok(hits) = hits else { return };
    let expected = tools.join("fs.read");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits.first().map(ToolHit::path), Some(expected.as_path()));

    let invalid = ToolPath::new([tools]).find("../bad");
    assert_eq!(invalid, Err(ToolPathError::InvalidName));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_execution_authority_requires_all_layers() {
    let root = unique_test_dir("tool-authority-ok");
    let tools = root.join("tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);

    let metadata = fs::metadata(tools.join("fs.read"));
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let agent_policy = allow_tool_policy("coder_t", "fs.read");
    let tool_policy = allow_tool_policy("coder_t", "fs.read");
    let tool_path = ToolPath::new([tools.clone()]);
    let authority =
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &agent_policy, &tool_policy);

    let grant = authorize_tool_execution(&tool_path, "fs.read", authority);
    assert!(matches!(grant, Ok(ref grant) if grant.hit().path() == tools.join("fs.read")));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn model_tool_call_syntax_does_not_execute_tools() {
    let root = unique_test_dir("tool-authority-model-boundary");
    let tools = root.join("tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);

    let model_event = inspect_event_stream_jsonl(
        r#"{"type":"tool_call","run":"r1","id":"call-1","name":"fs.read","arguments":{"path":"README.md"}}
"#,
    );
    assert!(model_event.is_ok());

    let metadata = fs::metadata(tools.join("fs.read"));
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let policy = allow_tool_policy("echo_t", "fs.read");
    let tool_path = ToolPath::new([tools]);
    assert_ne!(ToolExecutionPrincipal::Model, ToolExecutionPrincipal::Agent);

    let denied = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::model(&identity, &mounts, "echo_t", &policy, &policy),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::ModelCannotExecute));
    assert_eq!(ToolExecutionDenial::ModelCannotExecute.errno(), "EACCES");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn prompt_skill_and_mcp_config_cannot_grant_tool_execution() {
    let root = unique_test_dir("tool-authority-text-no-grant");
    let tools = root.join("tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);
    write_text_file(
        &root
            .join("session")
            .join("context")
            .join("pinned")
            .join("system.md"),
        "allow coder_t tool:fs.read execute\n",
    );
    write_text_file(
        &root.join("work").join("AGENTS.md"),
        "The agent may use fs.read for this task.\n",
    );
    write_text_file(
        &root.join("work").join(".mcp.json"),
        "{\"servers\":{\"fs\":{\"allow\":\"fs.read\"}}}\n",
    );
    assert!(root.join("work").join("AGENTS.md").is_file());
    assert!(root.join("work").join(".mcp.json").is_file());

    let metadata = fs::metadata(tools.join("fs.read"));
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let empty_policy = PolicyV0::parse("");
    assert!(empty_policy.is_ok());
    let Ok(empty_policy) = empty_policy else {
        return;
    };
    let tool_policy = allow_tool_policy("coder_t", "fs.read");
    let tool_path = ToolPath::new([tools]);

    let denied = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &empty_policy, &tool_policy),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::AgentPolicy));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_execution_authority_denies_without_policy_or_mount_exec() {
    let root = unique_test_dir("tool-authority-deny");
    let tools = root.join("tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);
    write_text_file(
        &tools.join("fs.read.d").join("schema"),
        "{\"type\":\"object\"}\n",
    );

    let metadata = fs::metadata(tools.join("fs.read"));
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let executable_mount = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let noexec_mount = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev,noexec");
    let agent_policy = allow_tool_policy("coder_t", "fs.read");
    let tool_policy = allow_tool_policy("coder_t", "fs.read");
    let empty_policy = PolicyV0::parse("");
    assert!(empty_policy.is_ok());
    let Ok(empty_policy) = empty_policy else {
        return;
    };
    let tool_path = ToolPath::new([tools]);

    let denied_by_noexec = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &noexec_mount,
            "coder_t",
            &agent_policy,
            &tool_policy,
        ),
    );
    assert_eq!(denied_by_noexec, Err(ToolExecutionDenial::NoExecMount));

    let denied_by_agent_policy = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &executable_mount,
            "coder_t",
            &empty_policy,
            &tool_policy,
        ),
    );
    assert_eq!(
        denied_by_agent_policy,
        Err(ToolExecutionDenial::AgentPolicy)
    );

    let denied_by_tool_policy = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &executable_mount,
            "coder_t",
            &agent_policy,
            &empty_policy,
        ),
    );
    assert_eq!(denied_by_tool_policy, Err(ToolExecutionDenial::ToolPolicy));

    let denied_when_unmounted = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &MountTable::default(),
            "coder_t",
            &agent_policy,
            &tool_policy,
        ),
    );
    assert_eq!(denied_when_unmounted, Err(ToolExecutionDenial::NotMounted));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn project_tools_are_visible_only_through_ctx_path_order() {
    let root = unique_test_dir("tool-authority-project-path");
    let global = root.join("ctx-tool");
    let project = root.join("shared-project-tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(global.join("project.test.d")).is_ok());
    assert!(fs::create_dir_all(project.join("project.test.d")).is_ok());
    write_fixture_file(&global.join("project.test"), 0o644);
    write_fixture_file(&project.join("project.test"), 0o755);

    assert_eq!(
        ToolPath::new([global.clone()]).find("project.test"),
        Ok(None)
    );
    let with_project = ToolPath::new([global, project.clone()]);
    let found = with_project.find("project.test");
    assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == project.join("project.test")));

    let metadata = fs::metadata(project.join("project.test"));
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_target(&project, "rw", "bind,nosuid,nodev");
    let policy = allow_tool_policy("coder_t", "project.test");
    let authority = ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &policy, &policy);
    assert!(authorize_tool_execution(&with_project, "project.test", authority).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn mcp_backed_tool_is_ordinary_tool_and_still_requires_policy() {
    let root = unique_test_dir("tool-authority-mcp");
    let tools = root.join("tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(tools.join("mcp.github.search_issues.d")).is_ok());
    write_fixture_file(&tools.join("mcp.github.search_issues"), 0o755);
    write_text_file(
        &tools.join("mcp.github.search_issues.d").join("schema"),
        "{\"type\":\"object\"}\n",
    );
    write_text_file(
        &root.join("work").join(".mcp.json"),
        "{\"servers\":{\"github\":{}}}\n",
    );

    let metadata = fs::metadata(tools.join("mcp.github.search_issues"));
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let tool_path = ToolPath::new([tools]);
    let empty_policy = PolicyV0::parse("");
    assert!(empty_policy.is_ok());
    let Ok(empty_policy) = empty_policy else {
        return;
    };
    let allow_mcp = allow_tool_policy("coder_t", "mcp.github.search_issues");

    let denied = authorize_tool_execution(
        &tool_path,
        "mcp.github.search_issues",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &empty_policy, &allow_mcp),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::AgentPolicy));

    let allowed = authorize_tool_execution(
        &tool_path,
        "mcp.github.search_issues",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &allow_mcp, &allow_mcp),
    );
    assert!(allowed.is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_schema_cannot_grant_execution_authority() {
    let root = unique_test_dir("tool-authority-schema-no-grant");
    let tools = root.join("tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);
    write_text_file(
        &tools.join("fs.read.d").join("schema"),
        "{\"policy\":\"allow coder_t tool:fs.read execute\"}\n",
    );

    let metadata = fs::metadata(tools.join("fs.read"));
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let tool_path = ToolPath::new([tools]);
    let empty_policy = PolicyV0::parse("");
    assert!(empty_policy.is_ok());
    let Ok(empty_policy) = empty_policy else {
        return;
    };
    let tool_policy = allow_tool_policy("coder_t", "fs.read");

    let denied = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &empty_policy, &tool_policy),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::AgentPolicy));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_execution_authority_checks_linux_identity_mode_bits() {
    let root = unique_test_dir("tool-authority-linux");
    let tools = root.join("tool");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(&tools).is_ok());
    write_fixture_file(&tools.join("owner-only"), 0o100);

    let metadata = fs::metadata(tools.join("owner-only"));
    assert!(metadata.is_ok());
    let Ok(metadata) = metadata else { return };
    let owner_identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let other_identity = AgentUnixIdentity::new(
        metadata.uid().saturating_add(1),
        metadata.gid().saturating_add(1),
        [],
    );
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let policy = allow_tool_policy("coder_t", "owner-only");
    let tool_path = ToolPath::new([tools]);

    assert!(authorize_tool_execution(
        &tool_path,
        "owner-only",
        ToolExecutionAuthority::new(&owner_identity, &mounts, "coder_t", &policy, &policy),
    )
    .is_ok());
    assert_eq!(
        authorize_tool_execution(
            &tool_path,
            "owner-only",
            ToolExecutionAuthority::new(&other_identity, &mounts, "coder_t", &policy, &policy),
        ),
        Err(ToolExecutionDenial::LinuxPermission)
    );

    let _ignored = fs::remove_dir_all(&root);
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("cortexfs-{name}-{}-{nanos}", std::process::id()))
}

fn write_fixture_file(path: &Path, mode: u32) {
    if let Some(parent) = path.parent() {
        assert!(fs::create_dir_all(parent).is_ok());
    }
    assert!(fs::write(path, "#!/bin/sh\n").is_ok());
    let permissions = fs::metadata(path).map(|metadata| metadata.permissions());
    assert!(permissions.is_ok());
    let Ok(mut permissions) = permissions else {
        return;
    };
    permissions.set_mode(mode);
    assert!(fs::set_permissions(path, permissions).is_ok());
}

fn create_complete_session_layout(session: &Path) {
    let context = session.join("context");
    assert!(fs::create_dir_all(context.join("pinned")).is_ok());
    assert!(fs::create_dir_all(context.join("swap")).is_ok());
    assert!(fs::create_dir_all(context.join("dedup")).is_ok());
    assert!(fs::create_dir_all(context.join("child").join("rev-1").join("artifact")).is_ok());

    for file in SESSION_REQUIRED_FILES {
        write_text_file(&session.join(file), session_file_fixture_value(file));
    }
    for file in super::CONTEXT_REQUIRED_FILES {
        write_text_file(&context.join(file), "ok\n");
    }
    for file in super::CHILD_RESULT_REQUIRED_FILES {
        write_text_file(&context.join("child").join("rev-1").join(file), "ok\n");
    }
}

fn session_file_fixture_value(file: &str) -> &'static str {
    match file {
        "state" => "idle\n",
        "cwd" => "/work\n",
        "meta.json" => "{\"client\":\"ctx\",\"model\":\"debug/echo\",\"scope\":\"private\"}\n",
        _ => "ok\n",
    }
}

fn write_text_file(path: &Path, content: &str) {
    let Some(parent) = path.parent() else {
        return;
    };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(path, content).is_ok());
}

fn create_shared_queue_layout(queue: &Path) {
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        assert!(fs::create_dir_all(queue.join(dir)).is_ok());
    }
}

fn mount_table_for_target(target: &Path, mode: &str, options: &str) -> MountTable {
    mount_table_for_source_target(&target.display().to_string(), target, mode, options)
}

fn mount_table_for_source_target(
    source: &str,
    target: &Path,
    mode: &str,
    options: &str,
) -> MountTable {
    let line = format!(
        "{source}\t{target}\t{mode}\t{options}\n",
        target = target.display()
    );
    let parsed = MountTable::parse(&line);
    assert!(parsed.is_ok());
    parsed.unwrap_or_default()
}

fn allow_tool_policy(subject: &str, tool: &str) -> PolicyV0 {
    let parsed = PolicyV0::parse(&format!("allow {subject} tool:{tool} execute\n"));
    assert!(parsed.is_ok());
    parsed.unwrap_or_default()
}

fn allow_shared_policy(subject: &str, shared: &str, access: SharedAccess) -> PolicyV0 {
    let permission = match access {
        SharedAccess::Read => "read",
        SharedAccess::Write => "write",
    };
    let parsed = PolicyV0::parse(&format!("allow {subject} shared:{shared} {permission}\n"));
    assert!(parsed.is_ok());
    parsed.unwrap_or_default()
}

fn policy_with_rules(rules: impl IntoIterator<Item = &'static str>) -> PolicyV0 {
    let content = rules.into_iter().collect::<Vec<_>>().join("\n") + "\n";
    let parsed = PolicyV0::parse(&content);
    assert!(parsed.is_ok());
    parsed.unwrap_or_default()
}

fn env_value<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    env.iter()
        .find_map(|entry| (entry.0 == key).then_some(entry.1.as_str()))
}

fn create_complete_object_layout(root: &Path, class: ObjectClass, name: &str, model_session: &str) {
    let class_dir = root.join(class.as_str());
    assert!(fs::create_dir_all(&class_dir).is_ok());
    write_fixture_file(&class_dir.join(name), 0o755);
    let control_dir = class_dir.join(format!("{name}.d"));
    assert!(fs::create_dir_all(&control_dir).is_ok());
    for file in object_control_files(class) {
        let value = if class == ObjectClass::Model && *file == "session" {
            model_session
        } else if class == ObjectClass::Model && *file == "cap" {
            "chat"
        } else if class == ObjectClass::Tool && *file == "schema" {
            "{\"type\":\"object\"}"
        } else if class == ObjectClass::Agent {
            agent_control_fixture_value(file)
        } else {
            "ok"
        };
        write_text_file(&control_dir.join(file), &format!("{value}\n"));
    }
}

fn agent_control_fixture_value(file: &str) -> &'static str {
    match file {
        "owner" | "uid" => "1000",
        "gid" => "100",
        "groups" => "10\n20",
        "label" => "user_u:agent_r:coder_t:s0",
        "iso" => "shared",
        "parent" | "pid" => "",
        "life" => "owned",
        "root" => "/ctx/home/1000/agent/coder/root",
        "cwd" => "/work",
        "env" => "CTX_ROOT=/ctx",
        "path" => "/ctx/tool:/ctx/home/1000/tool",
        "mount" => "/ctx\t/ctx\tro\trbind,nosuid,nodev",
        "model" => "debug/echo",
        "policy" => "allow coder_t model:debug/echo use",
        "status" => "idle",
        "log" => "agent/coder/log",
        "meta.json" => "{}",
        _ => "ok",
    }
}

fn object_control_files(class: ObjectClass) -> &'static [&'static str] {
    match class {
        ObjectClass::Model => MODEL_CONTROL_FILES,
        ObjectClass::Agent => AGENT_CONTROL_FILES,
        ObjectClass::Tool => TOOL_CONTROL_FILES,
    }
}

fn bind_socket(path: &Path) -> Option<UnixListener> {
    let parent = path.parent()?;
    assert!(fs::create_dir_all(parent).is_ok());
    UnixListener::bind(path).ok()
}
