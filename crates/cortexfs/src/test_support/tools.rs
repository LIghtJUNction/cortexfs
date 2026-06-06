use crate::CortexFs;

#[test]
fn projection_exposes_unified_tools_with_invoke_dirs() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["tools", "count"])
            .and_then(crate::Node::content),
        Some("3\n")
    );
    assert_eq!(
        fs.lookup_path(["tools", "list"])
            .and_then(crate::Node::content),
        Some("shell.exec\nfilesystem.read\nmcp.local-fs.read_file\n")
    );
    for tool in ["shell.exec", "filesystem.read", "mcp.local-fs.read_file"] {
        assert!(
            fs.lookup_path(["tools", tool, "input_schema.json"])
                .and_then(crate::Node::content)
                .is_some_and(|schema| schema.contains("\"type\":\"object\"")),
            "tool input schema must be readable"
        );
        assert!(
            fs.lookup_path(["tools", tool, "output_schema.json"])
                .and_then(crate::Node::content)
                .is_some_and(|schema| schema.contains("\"type\":\"object\"")),
            "tool output schema must be readable"
        );
        assert!(
            fs.lookup_path(["tools", tool, "permissions"]).is_some(),
            "tool permissions must be visible"
        );
        assert!(
            fs.lookup_path(["tools", tool, "invoke", "inbox"]).is_some(),
            "tool invoke inbox must exist"
        );
        assert!(
            fs.lookup_path(["tools", tool, "invoke", "outbox"])
                .is_some(),
            "tool invoke outbox must exist"
        );
    }
    assert_eq!(
        fs.lookup_path(["tools", "mcp.local-fs.read_file", "kind"])
            .and_then(crate::Node::content),
        Some("mcp\n")
    );
}

#[test]
fn projection_exposes_tool_permissions_and_default_policy_allowlists() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["tools", "shell.exec", "permissions"])
            .and_then(crate::Node::content),
        Some("host.shell.exec\n")
    );
    assert_eq!(
        fs.lookup_path(["tools", "filesystem.read", "permissions"])
            .and_then(crate::Node::content),
        Some("host.fs.read\n")
    );
    assert_eq!(
        fs.lookup_path(["tools", "mcp.local-fs.read_file", "permissions"])
            .and_then(crate::Node::content),
        Some("mcp.local-fs.read_file\nhost.fs.read\n")
    );

    let user_tools = fs
        .lookup_path(["spaces", "users", "1000", "tools", "enabled"])
        .and_then(crate::Node::content)
        .unwrap_or_default();
    let agent_tools = fs
        .lookup_path(["agents", "helper", "policy", "allowed_tools"])
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
        "{\"path\":\"spaces/users/1000/threads/demo/messages.jsonl\"}\n",
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
    assert!(response.contains("\"path\":\"spaces/users/1000/threads/demo/messages.jsonl\""));
    assert!(response.contains("tool visible"));
    drop(runtime);
    let tool_calls = fs.node_content(fs.export_file_inode("tool_calls.jsonl")?)?;
    assert!(tool_calls.contains("\"tool\":\"filesystem.read\""));
    assert!(tool_calls.contains("\"status\":\"ok\""));
    assert!(tool_calls.contains("\"output\":"));
    assert!(tool_calls.contains("tool visible"));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"filesystem.read\"")
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
