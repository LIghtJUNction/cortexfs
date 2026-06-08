use crate::CortexFs;
use fuse3::FileType;

fn mcp_server_runtime_inode(fs: &CortexFs, name: &'static str) -> fuse3::Result<fuse3::Inode> {
    fs.resolve_path_inode(["mcp", "server", "local-fs", name])
}

fn mcp_workspace_runtime_inode(fs: &CortexFs, name: &'static str) -> fuse3::Result<fuse3::Inode> {
    fs.resolve_path_inode(["mcp", "resource", "local-fs", "workspace", name])
}

fn mcp_session_runtime_inode(fs: &CortexFs, name: &'static str) -> fuse3::Result<fuse3::Inode> {
    fs.resolve_path_inode(["mcp", "session", "local-fs.demo", name])
}

fn mcp_session_search_runtime_inode(
    fs: &CortexFs,
    name: &'static str,
) -> fuse3::Result<fuse3::Inode> {
    fs.resolve_path_inode(["mcp", "session", "local-fs.demo", "search", name])
}

fn mcp_server_runtime_content(fs: &CortexFs, name: &'static str) -> fuse3::Result<String> {
    fs.node_content(mcp_server_runtime_inode(fs, name)?)
}

fn mcp_workspace_runtime_content(fs: &CortexFs, name: &'static str) -> fuse3::Result<String> {
    fs.node_content(mcp_workspace_runtime_inode(fs, name)?)
}

fn mcp_session_runtime_content(fs: &CortexFs, name: &'static str) -> fuse3::Result<String> {
    fs.node_content(mcp_session_runtime_inode(fs, name)?)
}

fn mcp_session_search_runtime_content(fs: &CortexFs, name: &'static str) -> fuse3::Result<String> {
    fs.node_content(mcp_session_search_runtime_inode(fs, name)?)
}

fn assert_mcp_runtime_files_are_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    for (parent_path, names) in [
        (
            ["mcp", "server", "local-fs"].as_slice(),
            ["status", "pid"].as_slice(),
        ),
        (
            ["mcp", "resource", "local-fs", "workspace"].as_slice(),
            ["content", "refresh"].as_slice(),
        ),
        (
            ["mcp", "session", "local-fs.demo"].as_slice(),
            ["state", "summary.md", "transcript.jsonl"].as_slice(),
        ),
        (
            ["mcp", "session", "local-fs.demo", "search"].as_slice(),
            ["query", "results.jsonl"].as_slice(),
        ),
    ] {
        let parent = fs
            .tree
            .path_inode(parent_path)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let entries = fs.children(parent);
        for name in names {
            let mut static_path = parent_path.to_vec();
            static_path.push(name);
            assert!(
                fs.tree.path_inode(&static_path).is_none(),
                "MCP runtime file {} must not have a static path inode",
                static_path.join("/")
            );
            assert_eq!(
                entries
                    .iter()
                    .filter(|entry| entry.name.to_str() == Some(*name))
                    .count(),
                1,
                "MCP runtime directory must expose one {name} entry"
            );
        }
    }
    Ok(())
}

#[test]
fn projection_exposes_mcp_primary_indexes() {
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
        fs.lookup_path(["mcp", "session", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["mcp", "session", "list"])
            .and_then(crate::Node::content),
        Some("local-fs.demo\n")
    );
}

#[test]
fn projection_exposes_mcp_objects() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_mcp_runtime_files_are_runtime_owned(&fs)?;
    assert_eq!(mcp_session_runtime_content(&fs, "state")?, "idle\n");
    assert_eq!(
        mcp_session_runtime_content(&fs, "summary.md")?,
        "lines=0\nlast_entry=\n"
    );
    assert_eq!(mcp_session_runtime_content(&fs, "transcript.jsonl")?, "");
    assert_eq!(mcp_session_search_runtime_content(&fs, "query")?, "\n");
    assert_eq!(
        mcp_session_search_runtime_content(&fs, "results.jsonl")?,
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
        fs.lookup_path(["mcp", "server", "local-fs", "cap"])
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
    assert!(
        fs.lookup_path(["mcp", "session", "local-fs.demo", "search"])
            .is_some(),
        "mcp session search directory must exist"
    );
    Ok(())
}

#[test]
fn mcp_resource_refresh_updates_content_last_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let refresh = mcp_workspace_runtime_inode(&fs, "refresh")?;

    assert_eq!(
        mcp_workspace_runtime_content(&fs, "content")?,
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

    let content = mcp_workspace_runtime_content(&fs, "content")?;
    assert!(content.contains("workspace=available\n"));
    assert!(content.contains("entries=1\n"));
    assert!(content.contains("refreshed=1\n"));
    assert_eq!(mcp_session_runtime_content(&fs, "state")?, "refreshed\n");
    let transcript = mcp_session_runtime_content(&fs, "transcript.jsonl")?;
    assert!(transcript.contains("\"type\":\"resource_refresh\""));
    assert!(transcript.contains("\"resource\":\"local-fs/workspace\""));
    let summary = mcp_session_runtime_content(&fs, "summary.md")?;
    assert!(summary.contains("lines=1\n"));
    assert!(summary.contains("\"type\":\"resource_refresh\""));
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
fn mcp_session_search_indexes_transcript() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let refresh = mcp_workspace_runtime_inode(&fs, "refresh")?;
    let search = mcp_session_search_runtime_inode(&fs, "query")?;

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(runtime.write(refresh, 0, b"1\n")?, 2);
        assert_eq!(runtime.write(search, 0, b"resource_refresh\n")?, 17);
        drop(runtime);
    }

    let results = mcp_session_search_runtime_content(&fs, "results.jsonl")?;
    assert!(results.contains("\"source\":\"mcp/session/local-fs.demo/transcript.jsonl\""));
    assert!(results.contains("\"session\":\"mcp/session/local-fs.demo\""));
    assert!(results.contains("resource_refresh"));
    assert_eq!(
        fs.node_content(fs.control_file_inode("last_control")?)?,
        "mcp/session/local-fs.demo/search/query\n"
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"mcp.session.search\""));
    assert!(audit.contains("\"name\":\"query\""));
    assert!(audit.contains("\"event\":\"searched\""));
    Ok(())
}

#[test]
fn mcp_resource_refresh_rejects_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let refresh = mcp_workspace_runtime_inode(&fs, "refresh")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    assert!(runtime.write(refresh, 0, b"yes\n").is_err());
    assert!(runtime.write(refresh, 1, b"1\n").is_err());
    drop(runtime);
    Ok(())
}

#[test]
fn projection_exposes_mcp_control_and_session_socket_semantics() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    let start_inode = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let control = fs.path_inode(["mcp", "server", "local-fs", "control"])?;
        assert!(
            runtime.lookup_child(control, "reload").is_none(),
            "MCP server control exposes only the current ABI entries"
        );
        for name in ["start", "stop", "restart"] {
            assert!(
                fs.tree
                    .path_inode(&["mcp", "server", "local-fs", "control", name])
                    .is_none(),
                "MCP server control command {name} must not have a static path inode"
            );
        }
        runtime
            .lookup_child(control, "start")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?
    };
    let control = fs.path_inode(["mcp", "server", "local-fs", "control"])?;
    let entries = fs.children(control);
    for name in ["start", "stop", "restart"] {
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "MCP server control directory must expose one {name} entry"
        );
    }
    assert!(
        entries
            .iter()
            .any(|entry| entry.name.to_str() == Some("start") && entry.inode == start_inode),
        "MCP server start entry must resolve to the runtime inode"
    );
    assert_eq!(fs.node_attr(start_inode)?.perm, 0o222);
    assert_eq!(
        fs.node_content(start_inode),
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

        assert_eq!(mcp_server_runtime_content(&fs, "status")?, expected_status);
        assert_eq!(mcp_server_runtime_content(&fs, "pid")?, expected_pid);
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("mcp/server/local-fs/{control_name}\n")
        );
    }

    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"mcp.server.local-fs.control\""));
    assert!(audit.contains("\"name\":\"start\""));
    assert!(audit.contains("\"name\":\"restart\""));
    assert!(audit.contains("\"name\":\"stop\""));
    let transcript = mcp_session_runtime_content(&fs, "transcript.jsonl")?;
    assert!(transcript.contains("\"type\":\"server_control\""));
    assert!(transcript.contains("\"command\":\"start\""));
    assert!(transcript.contains("\"command\":\"stop\""));
    Ok(())
}

#[test]
fn mcp_server_control_nodes_reject_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let control = fs.path_inode(["mcp", "server", "local-fs", "control"])?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let start = runtime
        .lookup_child(control, "start")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

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
    assert!(steps.contains("\"type\":\"permission_check\""));
    assert!(steps.contains("\"decision\":\"allow\""));
    assert!(steps.contains("\"permission\":\"mcp.local-fs.read_file\""));
    assert!(steps.contains("\"type\":\"tool_call\""));
    assert!(steps.contains("\"type\":\"tool_result\""));
    assert!(steps.contains("\"tool\":\"mcp.local-fs.read_file\""));
    assert!(steps.contains("mcp visible"));
    drop(runtime);
    let agent_traces = fs.node_content(fs.export_file_inode("agent_traces.jsonl")?)?;
    assert!(agent_traces.contains("\"agent\":\"helper\""));
    assert!(agent_traces.contains("\"event\":\"permission_check\""));
    assert!(agent_traces.contains("\"event\":\"tool_call\""));
    assert!(agent_traces.contains("\"event\":\"tool_result\""));
    assert!(agent_traces.contains("\"tool\":\"mcp.local-fs.read_file\""));
    assert!(agent_traces.contains("\"permission\":\"mcp.local-fs.read_file\""));
    assert!(agent_traces.contains("mcp visible"));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"mcp.local-fs.read_file\"")
    );
    let transcript = mcp_session_runtime_content(&fs, "transcript.jsonl")?;
    assert!(transcript.contains("\"type\":\"permission_check\""));
    assert!(transcript.contains("\"decision\":\"allow\""));
    assert!(transcript.contains("\"permission\":\"mcp.local-fs.read_file\""));
    assert!(transcript.contains("\"type\":\"tool_call\""));
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
    let transcript = mcp_session_runtime_content(&fs, "transcript.jsonl")?;
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
