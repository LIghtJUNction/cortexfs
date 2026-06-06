use crate::CortexFs;
use fuse3::FileType;

#[test]
fn projection_exposes_mcp_primary_and_compat_indexes() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["mcp", "server", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "server", "list"])
            .and_then(crate::Node::content),
        Some("local-fs\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "servers", "list"])
            .and_then(crate::Node::content),
        Some("local-fs\n"),
        "mcp servers compatibility path must expose the same index content"
    );
    assert_eq!(
        fs.lookup_path(["mcp", "tool", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "tool", "list"])
            .and_then(crate::Node::content),
        Some("local-fs.read_file\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "tools", "list"])
            .and_then(crate::Node::content),
        Some("local-fs.read_file\n"),
        "mcp tools compatibility path must expose the same index content"
    );
    assert_eq!(
        fs.lookup_path(["mcp", "resource", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "resource", "list"])
            .and_then(crate::Node::content),
        Some("local-fs/workspace\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "resources", "list"])
            .and_then(crate::Node::content),
        Some("local-fs/workspace\n"),
        "mcp resources compatibility path must expose the same index content"
    );
    assert_eq!(
        fs.lookup_path(["mcp", "prompt", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "prompt", "list"])
            .and_then(crate::Node::content),
        Some("local-fs/summarize-file\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "prompts", "list"])
            .and_then(crate::Node::content),
        Some("local-fs/summarize-file\n"),
        "mcp prompts compatibility path must expose the same index content"
    );
    assert_eq!(
        fs.lookup_path(["mcp", "session", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "session", "list"])
            .and_then(crate::Node::content),
        Some("local-fs.demo\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "sessions", "list"])
            .and_then(crate::Node::content),
        Some("local-fs.demo\n"),
        "mcp sessions compatibility path must expose the same index content"
    );
}

#[test]
fn projection_exposes_mcp_objects() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.node_content(fs.path_inode(["mcp", "session", "local-fs.demo", "state"])?)?,
        "idle\n"
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["mcp", "session", "local-fs.demo", "transcript.jsonl"])?)?,
        ""
    );
    assert_eq!(
        fs.lookup_path(["mcp", "server", "local-fs", "transport"])
            .and_then(crate::Node::content),
        Some("stdio\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "server", "local-fs", "context"])
            .and_then(crate::Node::content),
        Some("local:mcp_r:mcp_server_t:s0\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "server", "local-fs", "capabilities"])
            .and_then(crate::Node::content),
        Some("tools\nresources\nprompts\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "tool", "local-fs.read_file", "permissions"])
            .and_then(crate::Node::content),
        Some("host.fs.read\n")
    );
    assert!(
        fs.lookup_path(["mcp", "tool", "local-fs.read_file", "invoke", "inbox"])
            .is_some(),
        "mcp tool invoke inbox must exist"
    );
    assert_eq!(
        fs.lookup_path(["mcp", "resource", "local-fs", "workspace", "uri"])
            .and_then(crate::Node::content),
        Some("file://workspace\n")
    );
    assert!(
        fs.lookup_path([
            "mcp",
            "prompt",
            "local-fs",
            "summarize-file",
            "render",
            "outbox"
        ])
        .is_some(),
        "mcp prompt render outbox must exist"
    );
    Ok(())
}

#[test]
fn mcp_resource_refresh_updates_content_last_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let refresh = fs.path_inode(["mcp", "resource", "local-fs", "workspace", "refresh"])?;

    assert_eq!(
        fs.node_content(fs.path_inode(["mcp", "resource", "local-fs", "workspace", "content"])?)?,
        "workspace=available\nentries=0\n"
    );
    assert_eq!(fs.node_attr(refresh)?.perm, 0o222);
    assert_eq!(
        fs.node_content(refresh),
        Err(fuse3::Errno::from(libc::EACCES))
    );
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(runtime.write(refresh, 0, b"1\n")?, 2);
        drop(runtime);
    }

    let content =
        fs.node_content(fs.path_inode(["mcp", "resource", "local-fs", "workspace", "content"])?)?;
    assert!(content.contains("workspace=available\n"));
    assert!(content.contains("entries=1\n"));
    assert!(content.contains("refreshed=1\n"));
    assert_eq!(
        fs.node_content(fs.path_inode(["mcp", "session", "local-fs.demo", "state"])?)?,
        "refreshed\n"
    );
    let transcript =
        fs.node_content(fs.path_inode(["mcp", "session", "local-fs.demo", "transcript.jsonl"])?)?;
    assert!(transcript.contains("\"type\":\"resource_refresh\""));
    assert!(transcript.contains("\"resource\":\"local-fs/workspace\""));
    assert_eq!(
        fs.node_content(fs.control_file_inode("last_control")?)?,
        "mcp/resource/local-fs/workspace/refresh\n"
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"mcp.resource.local-fs.workspace\""));
    assert!(audit.contains("\"name\":\"refresh\""));
    assert!(audit.contains("\"event\":\"refreshed\""));
    Ok(())
}

#[test]
fn mcp_resource_refresh_rejects_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let refresh = fs.path_inode(["mcp", "resource", "local-fs", "workspace", "refresh"])?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    assert!(runtime.write(refresh, 0, b"yes\n").is_err());
    assert!(runtime.write(refresh, 1, b"1\n").is_err());
    drop(runtime);
    Ok(())
}

#[test]
fn projection_exposes_mcp_control_and_session_socket_semantics() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    let reload_inode = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(
                fs.path_inode(["mcp", "server", "local-fs", "control"])?,
                "reload",
            )
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?
    };
    assert_eq!(fs.node_attr(reload_inode)?.perm, 0o222);
    assert_eq!(
        fs.node_content(reload_inode),
        Err(fuse3::Errno::from(libc::EACCES))
    );
    assert_eq!(
        fs.lookup_path(["mcp", "session", "local-fs.demo", "io.sock"])
            .map(crate::Node::kind),
        Some(FileType::Socket)
    );
    let session_socket = fs
        .tree
        .path_inode(&["mcp", "session", "local-fs.demo", "io.sock"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        fs.node_content(session_socket).is_err(),
        "mcp session socket is a realtime endpoint, not a regular file"
    );
    Ok(())
}

#[test]
fn mcp_server_control_nodes_update_status_pid_last_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let control_dir = fs
        .tree
        .path_inode(&["mcp", "server", "local-fs", "control"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    for (control_name, expected_status, expected_pid) in [
        ("start", "running\n", "1234\n"),
        ("reload", "reloaded\n", "1234\n"),
        ("restart", "running\n", "1234\n"),
        ("stop", "stopped\n", "\n"),
    ] {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let control_inode = runtime
            .lookup_child(control_dir, control_name)
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        assert_eq!(runtime.write(control_inode, 0, b"1\n")?, 2);
        drop(runtime);

        assert_eq!(
            fs.node_content(fs.path_inode(["mcp", "server", "local-fs", "status"])?)?,
            expected_status
        );
        assert_eq!(
            fs.node_content(fs.path_inode(["mcp", "server", "local-fs", "pid"])?)?,
            expected_pid
        );
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("mcp/server/local-fs/{control_name}\n")
        );
    }

    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"mcp.server.local-fs.control\""));
    assert!(audit.contains("\"name\":\"start\""));
    assert!(audit.contains("\"name\":\"reload\""));
    assert!(audit.contains("\"name\":\"restart\""));
    assert!(audit.contains("\"name\":\"stop\""));
    let transcript =
        fs.node_content(fs.path_inode(["mcp", "session", "local-fs.demo", "transcript.jsonl"])?)?;
    assert!(transcript.contains("\"type\":\"server_control\""));
    assert!(transcript.contains("\"command\":\"start\""));
    assert!(transcript.contains("\"command\":\"stop\""));
    Ok(())
}

#[test]
fn mcp_server_control_nodes_reject_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let start = fs
        .tree
        .path_inode(&["mcp", "server", "local-fs", "control", "start"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    assert!(runtime.write(start, 0, b"yes\n").is_err());
    assert!(runtime.write(start, 1, b"1\n").is_err());
    drop(runtime);
    Ok(())
}

#[test]
fn mcp_local_fs_tool_invokes_through_unified_tool_plane() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_thread_request(
        "mcp-tool-source.tmp",
        "{\"messages\":[{\"role\":\"user\",\"content\":\"mcp visible\"}]}\n",
    )?;
    fs.submit_thread_request("mcp-tool-source.tmp", "mcp-tool-source.req.json")?;
    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    fs.create_staged_tool_request_at(
        crate::MCP_LOCAL_FS_READ_TOOL_INBOX_PATH,
        "mcp-read.tmp",
        "{\"path\":\"home/1000/thread/demo/messages.jsonl\"}\n",
    )?;
    fs.submit_tool_request_at(
        crate::MCP_LOCAL_FS_READ_TOOL_INBOX_PATH,
        "mcp-read.tmp",
        "mcp-read.req.json",
    )?;
    let outbox = fs
        .tree
        .path_inode(crate::MCP_LOCAL_FS_READ_TOOL_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert!(
            runtime.lookup_child(outbox, "mcp-read.resp.json").is_none(),
            "MCP tool submit queues work and does not run inside FUSE submit"
        );
        drop(runtime);
    }

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let response = runtime
        .lookup_child(outbox, "mcp-read.resp.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(response.contains("mcp visible"));
    drop(runtime);

    let tool_calls = fs.node_content(fs.export_file_inode("tool_calls.jsonl")?)?;
    assert!(tool_calls.contains("\"tool\":\"mcp.local-fs.read_file\""));
    assert!(tool_calls.contains("\"status\":\"ok\""));
    assert!(tool_calls.contains("mcp visible"));
    let tool_loop = fs
        .tree
        .path_inode(crate::DEMO_THREAD_TOOL_LOOP_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let steps = runtime
        .lookup_child(tool_loop, "steps.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(steps.contains("\"type\":\"tool_call\""));
    assert!(steps.contains("\"type\":\"tool_result\""));
    assert!(steps.contains("\"tool\":\"mcp.local-fs.read_file\""));
    assert!(steps.contains("mcp visible"));
    drop(runtime);
    let agent_traces = fs.node_content(fs.export_file_inode("agent_traces.jsonl")?)?;
    assert!(agent_traces.contains("\"agent\":\"helper\""));
    assert!(agent_traces.contains("\"event\":\"tool_call\""));
    assert!(agent_traces.contains("\"event\":\"tool_result\""));
    assert!(agent_traces.contains("\"tool\":\"mcp.local-fs.read_file\""));
    assert!(agent_traces.contains("mcp visible"));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"mcp.local-fs.read_file\"")
    );
    let transcript =
        fs.node_content(fs.path_inode(["mcp", "session", "local-fs.demo", "transcript.jsonl"])?)?;
    assert!(transcript.contains("\"type\":\"tool_result\""));
    assert!(transcript.contains("\"tool\":\"mcp.local-fs.read_file\""));
    assert!(transcript.contains("\"request_id\":\"mcp-read\""));
    Ok(())
}

#[test]
fn mcp_prompt_render_materializes_prompt_after_drain() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_prompt_render(
        "render-001.tmp",
        "{\"path\":\"home/1000/thread/demo/messages.jsonl\"}\n",
    )?;
    fs.submit_prompt_render("render-001.tmp", "render-001.req.json")?;
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );

    let outbox = fs
        .tree
        .path_inode(crate::MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert!(
            runtime
                .lookup_child(outbox, "render-001.resp.json")
                .is_none(),
            "prompt render queues work until control/drain"
        );
        drop(runtime);
    }

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let response = runtime
        .lookup_child(outbox, "render-001.resp.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(response.contains("\"prompt\":\"summarize-file\""));
    assert!(response.contains("Summarize the file at home/1000/thread/demo/messages.jsonl."));
    drop(runtime);
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"mcp.prompt.render\"")
    );
    let transcript =
        fs.node_content(fs.path_inode(["mcp", "session", "local-fs.demo", "transcript.jsonl"])?)?;
    assert!(transcript.contains("\"type\":\"prompt_render\""));
    assert!(transcript.contains("\"prompt\":\"summarize-file\""));
    assert!(transcript.contains("\"request_id\":\"render-001\""));
    Ok(())
}

#[test]
fn invalid_mcp_prompt_render_materializes_error() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_prompt_render("render-bad.tmp", "{}\n")?;
    fs.submit_prompt_render("render-bad.tmp", "render-bad.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(crate::MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let error = runtime
        .lookup_child(outbox, "render-bad.error")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(error.contains("missing path"));
    drop(runtime);
    Ok(())
}
