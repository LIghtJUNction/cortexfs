use crate::CortexFs;

#[test]
fn projection_exposes_unified_tools_with_invoke_dirs() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["tool", "count"])
            .and_then(crate::Node::content),
        Some("3\n")
    );
    assert_eq!(
        fs.lookup_path(["tool", "list"])
            .and_then(crate::Node::content),
        Some("shell.exec\nfilesystem.read\nmcp.local-fs.read_file\n")
    );
    for tool in ["shell.exec", "filesystem.read", "mcp.local-fs.read_file"] {
        assert!(
            fs.lookup_path(["tool", tool, "input_schema.json"])
                .and_then(crate::Node::content)
                .is_some_and(|schema| schema.contains("\"type\":\"object\"")),
            "tool input schema must be readable"
        );
        assert!(
            fs.lookup_path(["tool", tool, "output_schema.json"])
                .and_then(crate::Node::content)
                .is_some_and(|schema| schema.contains("\"type\":\"object\"")),
            "tool output schema must be readable"
        );
        assert!(
            fs.lookup_path(["tool", tool, "permissions"]).is_some(),
            "tool permissions must be visible"
        );
        assert!(
            fs.lookup_path(["tool", tool, "invoke", "inbox"]).is_some(),
            "tool invoke inbox must exist"
        );
        assert!(
            fs.lookup_path(["tool", tool, "invoke", "outbox"]).is_some(),
            "tool invoke outbox must exist"
        );
    }
    assert_eq!(
        fs.lookup_path(["tool", "mcp.local-fs.read_file", "kind"])
            .and_then(crate::Node::content),
        Some("mcp\n")
    );
}

#[test]
fn projection_exposes_tool_permissions_and_default_policy_allowlists() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["tool", "shell.exec", "permissions"])
            .and_then(crate::Node::content),
        Some("host.shell.exec\n")
    );
    assert_eq!(
        fs.lookup_path(["tool", "filesystem.read", "permissions"])
            .and_then(crate::Node::content),
        Some("host.fs.read\n")
    );
    assert_eq!(
        fs.lookup_path(["tool", "mcp.local-fs.read_file", "permissions"])
            .and_then(crate::Node::content),
        Some("mcp.local-fs.read_file\nhost.fs.read\n")
    );

    let user_tools = fs
        .lookup_path(["home", "1000", "tool", "enabled"])
        .and_then(crate::Node::content)
        .unwrap_or_default();
    let agent_tools = fs
        .lookup_path(["agent", "helper", "policy", "allowed_tools"])
        .and_then(crate::Node::content)
        .unwrap_or_default();

    for allowed in ["filesystem.read", "mcp.local-fs.read_file"] {
        assert!(
            user_tools.lines().any(|tool| tool == allowed),
            "space policy must expose default allowed tool: {allowed}"
        );
        assert!(
            agent_tools.lines().any(|tool| tool == allowed),
            "agent policy must expose default allowed tool: {allowed}"
        );
    }
    assert!(
        !user_tools.lines().any(|tool| tool == "shell.exec"),
        "shell.exec must be visible globally but not enabled in the default user space"
    );
    assert!(
        !agent_tools.lines().any(|tool| tool == "shell.exec"),
        "shell.exec must be visible globally but not allowed for the default agent"
    );
}

#[test]
fn filesystem_read_tool_invoke_materializes_response_after_drain() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_thread_request(
        "tool-source.tmp",
        "{\"messages\":[{\"role\":\"user\",\"content\":\"tool visible\"}]}\n",
    )?;
    fs.submit_thread_request("tool-source.tmp", "tool-source.req.json")?;
    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    fs.create_staged_tool_request(
        "read-001.tmp",
        "{\"path\":\"home/1000/thread/demo/messages.jsonl\"}\n",
    )?;
    fs.submit_tool_request("read-001.tmp", "read-001.req.json")?;
    let outbox = fs
        .tree
        .path_inode(crate::FILESYSTEM_READ_TOOL_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert!(
            runtime.lookup_child(outbox, "read-001.resp.json").is_none(),
            "tool rename queues work and does not execute until drain"
        );
        assert!(
            runtime
                .lookup_child(outbox, "read-001.route.json")
                .is_none(),
            "tool submissions must not expose provider route metadata"
        );
        drop(runtime);
    }
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let response = runtime
        .lookup_child(outbox, "read-001.resp.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(response.contains("\"path\":\"home/1000/thread/demo/messages.jsonl\""));
    assert!(response.contains("tool visible"));
    drop(runtime);
    let tool_calls = fs.node_content(fs.export_file_inode("tool_calls.jsonl")?)?;
    assert!(
        tool_calls.contains("\"source\":\"tool/filesystem.read/invoke/inbox/read-001.req.json\"")
    );
    assert!(tool_calls.contains("\"tool\":\"filesystem.read\""));
    assert!(tool_calls.contains("\"status\":\"ok\""));
    assert!(tool_calls.contains("\"output\":"));
    assert!(tool_calls.contains("tool visible"));
    let tool_loop = fs
        .tree
        .path_inode(crate::DEMO_THREAD_TOOL_LOOP_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let steps = runtime
        .lookup_child(tool_loop, "steps.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(steps.contains("\"type\":\"permission_check\""));
    assert!(steps.contains("\"decision\":\"allow\""));
    assert!(steps.contains("\"permission\":\"host.fs.read\""));
    assert!(steps.contains("\"policy\":\"agent/helper/policy/allowed_tools\""));
    drop(runtime);
    let agent_traces = fs.node_content(fs.export_file_inode("agent_traces.jsonl")?)?;
    assert!(
        agent_traces.contains("\"source\":\"tool/filesystem.read/invoke/inbox/read-001.req.json\"")
    );
    assert!(agent_traces.contains("\"event\":\"permission_check\""));
    assert!(agent_traces.contains("\"permission\":\"host.fs.read\""));
    assert!(agent_traces.contains("\"decision\":\"allow\""));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"filesystem.read\"")
    );
    Ok(())
}

#[test]
fn export_agent_filter_rebuilds_tool_and_agent_trace_views() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_thread_request(
        "tool-filter-source.tmp",
        "{\"messages\":[{\"role\":\"user\",\"content\":\"filter visible\"}]}\n",
    )?;
    fs.submit_thread_request("tool-filter-source.tmp", "tool-filter-source.req.json")?;
    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    fs.create_staged_tool_request(
        "filter-read.tmp",
        "{\"path\":\"home/1000/thread/demo/messages.jsonl\"}\n",
    )?;
    fs.submit_tool_request("filter-read.tmp", "filter-read.req.json")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let tool_calls = fs.export_file_inode("tool_calls.jsonl")?;
    let agent_traces = fs.export_file_inode("agent_traces.jsonl")?;
    assert!(fs.node_content(tool_calls)?.contains("filter visible"));
    assert!(fs.node_content(agent_traces)?.contains("filter visible"));

    let filter = fs
        .tree
        .path_inode(crate::EXPORT_FILTERS_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let agent = runtime
            .lookup_child(filter, "agent")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(agent, 0, b"other-agent\n")?;
    }
    assert_eq!(fs.node_content(tool_calls)?, crate::EMPTY_TEXT);
    assert_eq!(fs.node_content(agent_traces)?, crate::EMPTY_TEXT);

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let agent = runtime
            .lookup_child(filter, "agent")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(agent, 0, b"helper\n")?;
    }
    assert!(fs.node_content(tool_calls)?.contains("filter visible"));
    assert!(fs.node_content(agent_traces)?.contains("filter visible"));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"export.filter\"")
    );
    Ok(())
}

#[test]
fn tool_training_exports_dedupe_repeated_requests_by_fingerprint_group() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    for request_id in ["tool-dupe-a", "tool-dupe-b"] {
        fs.create_staged_tool_request(&format!("{request_id}.tmp"), "{\"path\":\"status\"}\n")?;
        fs.submit_tool_request(
            &format!("{request_id}.tmp"),
            &format!("{request_id}.req.json"),
        )?;
        let drain = fs.control_file_inode("drain")?;
        {
            let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
            runtime.write(drain, 0, b"1\n")?;
        }
    }

    let tool_calls = fs.node_content(fs.export_file_inode("tool_calls.jsonl")?)?;
    assert!(tool_calls.contains("\"request_id\":\"tool-dupe-a\""));
    assert!(!tool_calls.contains("\"request_id\":\"tool-dupe-b\""));
    assert_eq!(tool_calls.lines().count(), 1);

    let agent_traces = fs.node_content(fs.export_file_inode("agent_traces.jsonl")?)?;
    assert!(agent_traces.contains("\"request_id\":\"tool-dupe-a\""));
    assert!(!agent_traces.contains("\"request_id\":\"tool-dupe-b\""));
    assert!(agent_traces.contains("\"event\":\"permission_check\""));
    assert!(agent_traces.contains("\"event\":\"tool_call\""));
    assert!(agent_traces.contains("\"event\":\"tool_result\""));
    assert_eq!(
        agent_traces.lines().count(),
        3,
        "dedupe must keep the first complete trace group"
    );
    Ok(())
}

#[test]
fn tool_submit_is_not_blocked_by_provider_model_gate() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::PROVIDER_SPECS
        .first()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let enabled = fs.provider_child_dir_inode(provider.id, "enabled")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let current = runtime
            .lookup_child(enabled, "current")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(current, 0, b"0\n")?;
    }

    fs.create_staged_tool_request("read-status.tmp", "{\"path\":\"status\"}\n")?;
    fs.submit_tool_request("read-status.tmp", "read-status.req.json")?;
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }
    let outbox = fs
        .tree
        .path_inode(crate::FILESYSTEM_READ_TOOL_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let response = runtime
        .lookup_child(outbox, "read-status.resp.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(response.contains("\"content\":\"ready\\n\""));
    drop(runtime);
    Ok(())
}

#[test]
fn shell_exec_tool_is_denied_by_default_policy() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_tool_request_at(
        crate::SHELL_EXEC_TOOL_INBOX_PATH,
        "shell-deny.tmp",
        "{\"command\":\"echo should-not-run\"}\n",
    )?;
    fs.submit_tool_request_at(
        crate::SHELL_EXEC_TOOL_INBOX_PATH,
        "shell-deny.tmp",
        "shell-deny.req.json",
    )?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(crate::SHELL_EXEC_TOOL_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let tool_loop = fs
        .tree
        .path_inode(crate::DEMO_THREAD_TOOL_LOOP_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime
            .lookup_child(outbox, "shell-deny.resp.json")
            .is_none()
    );
    let error = runtime
        .lookup_child(outbox, "shell-deny.error")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(error.contains("\"status\":\"denied\""));
    assert!(error.contains("\"tool\":\"shell.exec\""));
    assert!(error.contains("\"permission\":\"host.shell.exec\""));
    let steps = runtime
        .lookup_child(tool_loop, "steps.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(steps.contains("\"type\":\"permission_check\""));
    assert!(steps.contains("\"decision\":\"deny\""));
    assert!(steps.contains("\"tool\":\"shell.exec\""));
    assert!(!steps.contains("\"type\":\"tool_call\""));
    assert!(!steps.contains("\"type\":\"tool_result\""));
    drop(runtime);

    let agent_traces = fs.node_content(fs.export_file_inode("agent_traces.jsonl")?)?;
    assert!(
        agent_traces.contains("\"source\":\"tool/shell.exec/invoke/inbox/shell-deny.req.json\"")
    );
    assert!(agent_traces.contains("\"event\":\"permission_check\""));
    assert!(agent_traces.contains("\"decision\":\"deny\""));
    assert!(agent_traces.contains("\"permission\":\"host.shell.exec\""));
    assert_eq!(
        fs.node_content(fs.export_file_inode("tool_calls.jsonl")?)?,
        ""
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"shell.exec\""));
    assert!(audit.contains("\"event\":\"denied\""));
    Ok(())
}
