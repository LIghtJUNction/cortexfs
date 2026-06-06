use crate::CortexFs;
use fuse3::FileType;

#[test]
fn projection_exposes_helper_agent_profile_runtime_and_socket() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["agents", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["agents", "list"])
            .and_then(crate::Node::content),
        Some("helper\n")
    );
    assert_eq!(
        fs.lookup_path(["agents", "helper", "profile", "name"])
            .and_then(crate::Node::content),
        Some("helper\n")
    );
    assert_eq!(
        fs.lookup_path(["agents", "helper", "context"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_AGENT_CONTEXT_TEXT)
    );
    assert_eq!(
        fs.lookup_path(["agents", "helper", "profile", "model", "provider"])
            .and_then(crate::Node::content),
        Some(format!("{}\n", crate::default_provider_id()).as_str())
    );
    assert_eq!(
        fs.lookup_path(["agents", "helper", "runtime", "state"])
            .and_then(crate::Node::content),
        Some("idle\n")
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "pid"])?)?,
        "\n"
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "heartbeat"])?)?,
        "\n"
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "current_thread"])?)?,
        "\n"
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "current_task"])?)?,
        "\n"
    );
    assert_eq!(
        fs.lookup_path(["agents", "helper", "io.sock"])
            .map(crate::Node::kind),
        Some(FileType::Socket)
    );
    assert_eq!(
        fs.lookup_path(["agents", "helper", "policy", "allowed_skills"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert!(
        fs.lookup_path(["agents", "helper", "inbox"]).is_some(),
        "agent file task inbox must exist"
    );
    assert!(
        fs.lookup_path(["agents", "helper", "outbox"]).is_some(),
        "agent file task outbox must exist"
    );
    assert!(
        fs.lookup_path(["agents", "helper", "control", "start"])
            .is_some(),
        "agent control start node must exist"
    );
    let socket_inode = fs
        .tree
        .path_inode(&["agents", "helper", "io.sock"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        fs.node_content(socket_inode).is_err(),
        "socket nodes are realtime endpoints, not regular files"
    );
    Ok(())
}

#[test]
fn projection_exposes_helper_agent_capability_views() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["agent", "helper", "skill", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["agent", "helper", "skill", "list"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert!(
        fs.lookup_path(["agent", "helper", "skills", "list"])
            .is_some()
    );
    assert_eq!(
        fs.lookup_path(["agent", "helper", "tool", "count"])
            .and_then(crate::Node::content),
        Some("2\n")
    );
    assert_eq!(
        fs.lookup_path(["agent", "helper", "tool", "list"])
            .and_then(crate::Node::content),
        Some("filesystem.read\nmcp.local-fs.read_file\n")
    );
    assert_eq!(
        fs.lookup_path(["agent", "helper", "tool", "enabled"])
            .and_then(crate::Node::content),
        Some("filesystem.read\nmcp.local-fs.read_file\n")
    );
    assert!(
        fs.lookup_path(["agent", "helper", "tools", "list"])
            .is_some()
    );
    assert_eq!(
        fs.lookup_path(["agent", "helper", "mcp", "server", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["agent", "helper", "mcp", "server", "list"])
            .and_then(crate::Node::content),
        Some("local-fs\n")
    );
    assert_eq!(
        fs.lookup_path(["agent", "helper", "mcp", "server", "enabled"])
            .and_then(crate::Node::content),
        Some("local-fs\n")
    );
    assert!(
        fs.lookup_path(["agent", "helper", "mcp", "servers", "enabled"])
            .is_some()
    );
}

#[test]
fn agent_helper_control_nodes_update_runtime_last_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for (control_name, expected_state, expected_pid, expected_heartbeat, expected_thread) in [
        (
            "start",
            "running\n",
            "1234\n",
            "1\n",
            crate::LOCAL_USER_THREAD_DISPLAY_TEXT,
        ),
        (
            "pause",
            "paused\n",
            "1234\n",
            "1\n",
            crate::LOCAL_USER_THREAD_DISPLAY_TEXT,
        ),
        (
            "restart",
            "running\n",
            "1234\n",
            "1\n",
            crate::LOCAL_USER_THREAD_DISPLAY_TEXT,
        ),
        ("stop", "stopped\n", "\n", "\n", "\n"),
    ] {
        let control_inode = fs.agent_helper_control_file_inode(control_name)?;
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        if let Some(task_inode) = runtime.agent_helper_runtime_current_task_inode {
            runtime.update_dynamic_file(task_inode, "stale-task\n");
        }
        assert_eq!(runtime.write(control_inode, 0, b"1\n")?, 2);
        drop(runtime);

        assert_eq!(
            fs.lookup_path(["agents", "helper", "runtime", "state"])
                .map(crate::Node::inode)
                .map(|inode| fs.node_content(inode))
                .transpose()?,
            Some(expected_state.to_owned())
        );
        assert_eq!(
            fs.node_content(fs.path_inode(["agents", "helper", "runtime", "pid"])?)?,
            expected_pid
        );
        assert_eq!(
            fs.node_content(fs.path_inode(["agents", "helper", "runtime", "heartbeat"])?)?,
            expected_heartbeat
        );
        assert_eq!(
            fs.node_content(fs.path_inode(["agents", "helper", "runtime", "current_thread"])?)?,
            expected_thread
        );
        let current_task = fs
            .lookup_path(["agents", "helper", "runtime", "current_task"])
            .map(crate::Node::inode)
            .map(|inode| fs.node_content(inode))
            .transpose()?
            .unwrap_or_default();
        if matches!(control_name, "start" | "stop" | "restart") {
            assert_eq!(current_task, "\n");
        }
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("agents/helper/{control_name}\n")
        );
        let audit = fs.node_content(fs.audit_events_inode()?)?;
        assert!(audit.contains("\"format\":\"agent.helper.control\""));
        assert!(audit.contains(&format!("\"name\":\"{control_name}\"")));
        assert!(audit.contains(&format!("\"event\":\"{}\"", expected_state.trim())));
    }
    Ok(())
}

#[test]
fn agent_helper_control_nodes_reject_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let start = fs.agent_helper_control_file_inode("start")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    assert!(runtime.write(start, 0, b"yes\n").is_err());
    assert!(runtime.write(start, 1, b"1\n").is_err());
    drop(runtime);
    Ok(())
}

#[test]
fn agent_inbox_submit_drains_to_outbox_and_trace() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_agent_task(
        "assist.tmp",
        "{\"task\":\"summarize\",\"input\":\"agent visible\"}\n",
    )?;
    fs.submit_agent_task("assist.tmp", "assist.req.json")?;

    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );
    assert_eq!(
        fs.lookup_path(["agents", "helper", "runtime", "state"])
            .map(crate::Node::inode)
            .map(|inode| fs.node_content(inode))
            .transpose()?,
        Some("busy\n".to_owned())
    );
    assert_eq!(
        fs.lookup_path(["agents", "helper", "runtime", "current_task"])
            .map(crate::Node::inode)
            .map(|inode| fs.node_content(inode))
            .transpose()?,
        Some("assist\n".to_owned())
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "pid"])?)?,
        "1234\n"
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "heartbeat"])?)?,
        "1\n"
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "current_thread"])?)?,
        crate::LOCAL_USER_THREAD_DISPLAY_TEXT
    );

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(crate::AGENT_HELPER_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime
            .lookup_child(outbox, "assist.resp.json")
            .and_then(crate::Node::content)
            .is_some_and(|content| {
                content.contains("\"agent\":\"helper\"")
                    && content.contains("\"status\":\"done\"")
                    && content.contains("agent visible")
            })
    );
    drop(runtime);
    assert_eq!(
        fs.lookup_path(["agents", "helper", "runtime", "state"])
            .map(crate::Node::inode)
            .map(|inode| fs.node_content(inode))
            .transpose()?,
        Some("idle\n".to_owned())
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "pid"])?)?,
        "1234\n"
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "heartbeat"])?)?,
        "2\n"
    );
    assert_eq!(
        fs.node_content(fs.path_inode(["agents", "helper", "runtime", "current_thread"])?)?,
        crate::LOCAL_USER_THREAD_DISPLAY_TEXT
    );
    let agent_traces = fs.node_content(fs.export_file_inode("agent_traces.jsonl")?)?;
    assert!(agent_traces.contains("\"event\":\"task\""));
    assert!(agent_traces.contains("\"event\":\"task_result\""));
    assert!(agent_traces.contains("agent visible"));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"agent.task\"")
    );
    assert_eq!(
        fs.node_content(fs.audit_cost_inode()?)?,
        "usd=0.000001\nbillable_events=1\ndrained=1\ntool_calls=0\nagent_tasks=1\n"
    );
    Ok(())
}
