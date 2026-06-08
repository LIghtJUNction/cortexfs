use crate::CortexFs;
use fuse3::FileType;
use fuse3::Inode;

fn assert_demo_thread_runtime_files_are_runtime_owned(
    fs: &CortexFs,
    thread: Inode,
) -> fuse3::Result<()> {
    assert_thread_runtime_files_are_runtime_owned(
        fs,
        thread,
        &["home", "1000", "thread", "demo"],
        "demo thread",
    )
}

fn assert_external_thread_runtime_files_are_runtime_owned(
    fs: &CortexFs,
    thread: Inode,
) -> fuse3::Result<()> {
    assert_thread_runtime_files_are_runtime_owned(
        fs,
        thread,
        &["ext", "qq", "group", "888888", "thread", "demo"],
        "external qq thread",
    )
}

fn assert_thread_runtime_files_are_runtime_owned(
    fs: &CortexFs,
    thread: Inode,
    static_parent_path: &[&str],
    label: &str,
) -> fuse3::Result<()> {
    let entries = fs.children(thread);
    for (file, expected) in [
        ("messages.jsonl", crate::EMPTY_TEXT),
        ("latest.md", crate::EMPTY_TEXT),
        ("state", "idle\n"),
        ("fingerprint", crate::EMPTY_TEXT),
    ] {
        let mut static_path = static_parent_path.to_vec();
        static_path.push(file);
        assert!(
            fs.tree.path_inode(&static_path).is_none(),
            "{label} {file} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(file))
                .count(),
            1,
            "{label} directory must expose one {file} entry"
        );
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime
            .lookup_child(thread, file)
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        drop(runtime);
        assert_eq!(fs.node_content(inode)?, expected);
    }
    Ok(())
}

fn assert_tool_loop_runtime_files_are_runtime_owned(
    fs: &CortexFs,
    tool_loop: Inode,
) -> fuse3::Result<()> {
    let entries = fs.children(tool_loop);
    for (file, expected) in [("state", "idle\n"), ("steps.jsonl", crate::EMPTY_TEXT)] {
        assert!(
            fs.tree
                .path_inode(&["home", "1000", "thread", "demo", "tool-loop", file])
                .is_none(),
            "demo tool-loop {file} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(file))
                .count(),
            1,
            "demo tool-loop directory must expose one {file} entry"
        );
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime
            .lookup_child(tool_loop, file)
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        drop(runtime);
        assert_eq!(fs.node_content(inode)?, expected);
    }
    Ok(())
}

fn assert_control_files_are_runtime_owned<const N: usize>(
    fs: &CortexFs,
    parent_path: [&str; N],
    label: &str,
) -> fuse3::Result<()> {
    let control = fs.resolve_path_inode(parent_path)?;
    let entries = fs.children(control);
    for name in ["continue", "pause", "cancel"] {
        let mut child_path = parent_path.map(str::to_owned).to_vec();
        child_path.push(name.to_owned());
        assert!(
            fs.tree.path_inode_owned(&child_path).is_none(),
            "{label} control {name} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "{label} control directory must expose one {name} entry"
        );
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime
            .lookup_child(control, name)
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        drop(runtime);
        assert_eq!(fs.node_attr(inode)?.perm, 0o222);
    }
    Ok(())
}

#[test]
fn projection_exposes_demo_thread_and_tool_loop_state() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["home", "1000", "thread", "count"])
            .and_then(crate::Node::content),
        Some(crate::THREAD_COUNT_TEXT)
    );
    assert!(
        fs.lookup_path(["home", "1000", "thread", "demo", "inbox"])
            .is_some(),
        "thread inbox must exist"
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "thread", "demo", "io.sock"])
            .map(crate::Node::kind),
        Some(FileType::Socket)
    );
    let thread = fs
        .tree
        .path_inode(crate::DEMO_THREAD_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_demo_thread_runtime_files_are_runtime_owned(&fs, thread)?;
    assert!(
        fs.lookup_path(["home", "1000", "thread", "demo", "inbox"])
            .is_some(),
        "thread inbox must exist"
    );
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let thread = fs
        .tree
        .path_inode(crate::DEMO_THREAD_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(
        runtime
            .lookup_child(thread, "messages.jsonl")
            .and_then(crate::Node::content),
        Some(crate::EMPTY_TEXT)
    );
    assert_eq!(
        runtime
            .lookup_child(thread, "state")
            .and_then(crate::Node::content),
        Some("idle\n")
    );
    drop(runtime);
    let tool_loop = fs
        .tree
        .path_inode(crate::DEMO_THREAD_TOOL_LOOP_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_tool_loop_runtime_files_are_runtime_owned(&fs, tool_loop)?;
    for (limit_name, expected) in [
        ("max_steps", "64\n"),
        ("max_time_ms", "300000\n"),
        ("max_cost_usd", "0.10\n"),
    ] {
        assert_eq!(
            fs.lookup_path([
                "home",
                "1000",
                "thread",
                "demo",
                "tool-loop",
                "limit",
                limit_name,
            ])
            .and_then(crate::Node::content),
            Some(expected)
        );
    }
    assert_control_files_are_runtime_owned(
        &fs,
        ["home", "1000", "thread", "demo", "control"],
        "demo thread",
    )?;
    assert_control_files_are_runtime_owned(
        &fs,
        ["home", "1000", "thread", "demo", "tool-loop", "control"],
        "demo tool-loop",
    )?;
    Ok(())
}

#[test]
fn demo_tool_loop_limit_nodes_update_values_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for (limit_name, new_value) in [
        ("max_steps", "128\n"),
        ("max_time_ms", "600000\n"),
        ("max_cost_usd", "0.25\n"),
    ] {
        let inode = fs.demo_tool_loop_limit_file_inode(limit_name)?;
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(
            runtime.write(inode, 0, new_value.as_bytes())?,
            u32::try_from(new_value.len()).map_err(|_error| libc::EIO)?
        );
        drop(runtime);

        assert_eq!(fs.node_content(inode)?, new_value);
        let audit = fs.node_content(fs.audit_events_inode()?)?;
        assert!(audit.contains("\"format\":\"tool-loop.demo.limits\""));
        assert!(audit.contains(&format!("\"name\":\"{limit_name}\"")));
        assert!(audit.contains("\"event\":\"configured\""));
    }
    Ok(())
}

#[test]
fn demo_tool_loop_limit_nodes_reject_invalid_values() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let max_steps = fs.demo_tool_loop_limit_file_inode("max_steps")?;
    let max_time_ms = fs.demo_tool_loop_limit_file_inode("max_time_ms")?;
    let max_cost_usd = fs.demo_tool_loop_limit_file_inode("max_cost_usd")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    assert!(runtime.write(max_steps, 0, b"0\n").is_err());
    assert!(runtime.write(max_steps, 0, b"-1\n").is_err());
    assert!(runtime.write(max_time_ms, 0, b"forever\n").is_err());
    assert!(runtime.write(max_cost_usd, 0, b"-0.1\n").is_err());
    assert!(runtime.write(max_cost_usd, 0, b"1.2.3\n").is_err());
    assert!(runtime.write(max_cost_usd, 1, b"0.20\n").is_err());
    drop(runtime);

    assert_eq!(fs.node_content(max_steps)?, "64\n");
    assert_eq!(fs.node_content(max_time_ms)?, "300000\n");
    assert_eq!(fs.node_content(max_cost_usd)?, "0.10\n");
    Ok(())
}

#[test]
fn demo_tool_loop_max_steps_limits_appended_steps() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let max_steps = fs.demo_tool_loop_limit_file_inode("max_steps")?;
    let continue_control = fs.demo_tool_loop_control_file_inode("continue")?;
    let pause_control = fs.demo_tool_loop_control_file_inode("pause")?;

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(max_steps, 0, b"1\n")?;
        runtime.write(continue_control, 0, b"1\n")?;
        runtime.write(pause_control, 0, b"1\n")?;
    }

    assert_eq!(
        fs.node_content(fs.demo_tool_loop_runtime_file_inode("state")?)?,
        "limit_exceeded\n"
    );
    let steps = fs.node_content(fs.demo_tool_loop_runtime_file_inode("steps.jsonl")?)?;
    assert_eq!(steps.lines().count(), 1);
    assert!(steps.contains("\"command\":\"continue\""));
    assert!(!steps.contains("\"command\":\"pause\""));
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"tool-loop.demo.limits\""));
    assert!(audit.contains("\"name\":\"max_steps\""));
    assert!(audit.contains("\"event\":\"exceeded\""));
    Ok(())
}

#[test]
fn demo_tool_loop_max_time_limits_appended_steps() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let max_time_ms = fs.demo_tool_loop_limit_file_inode("max_time_ms")?;
    let continue_control = fs.demo_tool_loop_control_file_inode("continue")?;
    let pause_control = fs.demo_tool_loop_control_file_inode("pause")?;

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(max_time_ms, 0, b"1\n")?;
        runtime.write(continue_control, 0, b"1\n")?;
        runtime.tool_loop_started_at =
            std::time::Instant::now().checked_sub(std::time::Duration::from_millis(2));
        assert!(runtime.tool_loop_started_at.is_some());
        runtime.write(pause_control, 0, b"1\n")?;
    }

    assert_eq!(
        fs.node_content(fs.demo_tool_loop_runtime_file_inode("state")?)?,
        "limit_exceeded\n"
    );
    let steps = fs.node_content(fs.demo_tool_loop_runtime_file_inode("steps.jsonl")?)?;
    assert_eq!(steps.lines().count(), 1);
    assert!(steps.contains("\"command\":\"continue\""));
    assert!(!steps.contains("\"command\":\"pause\""));
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"tool-loop.demo.limits\""));
    assert!(audit.contains("\"name\":\"max_time_ms\""));
    assert!(audit.contains("\"event\":\"exceeded\""));
    Ok(())
}

#[test]
fn demo_tool_loop_max_cost_limits_appended_steps() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let max_cost_usd = fs.demo_tool_loop_limit_file_inode("max_cost_usd")?;
    let continue_control = fs.demo_tool_loop_control_file_inode("continue")?;
    let pause_control = fs.demo_tool_loop_control_file_inode("pause")?;

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(max_cost_usd, 0, b"0.000001\n")?;
        runtime.write(continue_control, 0, b"1\n")?;
        runtime.write(pause_control, 0, b"1\n")?;
    }

    assert_eq!(
        fs.node_content(fs.demo_tool_loop_runtime_file_inode("state")?)?,
        "limit_exceeded\n"
    );
    let steps = fs.node_content(fs.demo_tool_loop_runtime_file_inode("steps.jsonl")?)?;
    assert_eq!(steps.lines().count(), 1);
    assert!(steps.contains("\"command\":\"continue\""));
    assert!(!steps.contains("\"command\":\"pause\""));
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"tool-loop.demo.limits\""));
    assert!(audit.contains("\"name\":\"max_cost_usd\""));
    assert!(audit.contains("\"event\":\"exceeded\""));
    Ok(())
}

#[test]
fn demo_thread_control_nodes_update_state_last_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for (control_name, expected_state) in [
        ("continue", "running\n"),
        ("pause", "paused\n"),
        ("cancel", "cancelled\n"),
    ] {
        let control_inode = fs.demo_thread_control_file_inode(control_name)?;
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(runtime.write(control_inode, 0, b"1\n")?, 2);
        drop(runtime);

        assert_eq!(
            fs.node_content(fs.demo_thread_runtime_file_inode("state")?)?,
            expected_state
        );
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("home/1000/thread/demo/{control_name}\n")
        );
        let audit = fs.node_content(fs.audit_events_inode()?)?;
        assert!(audit.contains("\"format\":\"thread.demo.control\""));
        assert!(audit.contains(&format!("\"name\":\"{control_name}\"")));
        assert!(audit.contains(&format!("\"event\":\"{}\"", expected_state.trim())));
    }
    Ok(())
}

#[test]
fn demo_tool_loop_control_nodes_update_state_steps_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for (control_name, expected_state) in [
        ("continue", "running\n"),
        ("pause", "paused\n"),
        ("cancel", "cancelled\n"),
    ] {
        let control_inode = fs.demo_tool_loop_control_file_inode(control_name)?;
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(runtime.write(control_inode, 0, b"1\n")?, 2);
        drop(runtime);

        assert_eq!(
            fs.node_content(fs.demo_tool_loop_runtime_file_inode("state")?)?,
            expected_state
        );
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("home/1000/thread/demo/tool-loop/{control_name}\n")
        );
        let steps = fs.node_content(fs.demo_tool_loop_runtime_file_inode("steps.jsonl")?)?;
        assert!(steps.contains("\"type\":\"control\""));
        assert!(steps.contains(&format!("\"command\":\"{control_name}\"")));
        assert!(steps.contains(&format!("\"state\":\"{}\"", expected_state.trim())));
        let audit = fs.node_content(fs.audit_events_inode()?)?;
        assert!(audit.contains("\"format\":\"tool-loop.demo.control\""));
        assert!(audit.contains(&format!("\"name\":\"{control_name}\"")));
    }
    Ok(())
}

#[test]
fn demo_thread_and_tool_loop_control_nodes_reject_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let thread_pause = fs.demo_thread_control_file_inode("pause")?;
    let tool_loop_pause = fs.demo_tool_loop_control_file_inode("pause")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    assert!(runtime.write(thread_pause, 0, b"yes\n").is_err());
    assert!(runtime.write(thread_pause, 1, b"1\n").is_err());
    assert!(runtime.write(tool_loop_pause, 0, b"yes\n").is_err());
    assert!(runtime.write(tool_loop_pause, 1, b"1\n").is_err());
    drop(runtime);
    Ok(())
}

#[test]
fn thread_inbox_submit_updates_messages_after_drain() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_thread_request(
        "turn-001.tmp",
        "{\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}]}\n",
    )?;
    fs.submit_thread_request("turn-001.tmp", "turn-001.req.json")?;

    let thread = fs
        .tree
        .path_inode(crate::DEMO_THREAD_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(
            runtime
                .lookup_child(thread, "state")
                .and_then(crate::Node::content),
            Some("queued\n")
        );
        assert_eq!(
            runtime
                .lookup_child(thread, "messages.jsonl")
                .and_then(crate::Node::content),
            Some(crate::EMPTY_TEXT),
            "rename queues work but does not run the provider in FUSE"
        );
        assert!(
            runtime
                .lookup_child(thread, "fingerprint")
                .and_then(crate::Node::content)
                .is_some_and(|fingerprint| fingerprint.starts_with("fnv1a64:"))
        );
        drop(runtime);
    }

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let messages = runtime
        .lookup_child(thread, "messages.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(messages.contains("\"role\":\"user\""));
    assert!(messages.contains("\"content\":\"ping\""));
    assert!(messages.contains("\"role\":\"assistant\""));
    assert!(messages.contains("\"content\":\"cortexfs-ok\""));
    assert!(
        runtime.lookup_child(thread, "turn-001.resp.json").is_none(),
        "thread responses update messages/latest instead of creating inbox responses"
    );
    assert_eq!(
        runtime
            .lookup_child(thread, "latest.md")
            .and_then(crate::Node::content),
        Some("cortexfs-ok\n")
    );
    assert_eq!(
        runtime
            .lookup_child(thread, "state")
            .and_then(crate::Node::content),
        Some("idle\n")
    );
    let episodic = fs.path_inode(["home", "1000", "memory", "episodic"])?;
    let episodic_items = runtime
        .lookup_child(episodic, "items.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(episodic_items.contains("thread=home/1000/thread/demo"));
    assert!(episodic_items.contains("ping"));
    assert!(episodic_items.contains("cortexfs-ok"));
    drop(runtime);
    let steps = fs.node_content(fs.demo_tool_loop_runtime_file_inode("steps.jsonl")?)?;
    assert!(steps.contains("\"step\":1"));
    assert!(steps.contains("\"type\":\"model\""));
    assert!(steps.contains("\"message\":\"cortexfs-ok\""));
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"turn-001\""));
    assert!(export.contains("\"source\":\"home/1000/thread/demo/inbox/turn-001.req.json\""));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"name\":\"turn-001.req.json\"")
    );
    Ok(())
}

#[test]
fn external_thread_submit_preserves_subject_identity() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    assert!(
        fs.tree
            .path_inode(crate::EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH)
            .is_none(),
        "external subject quota requests must be runtime-owned, not a static placeholder"
    );
    let quota = fs.resolve_path_inode(crate::EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH)?;
    assert_eq!(fs.node_content(quota)?, "0\n");
    let thread = fs
        .tree
        .path_inode(crate::EXTERNAL_QQ_GROUP_THREAD_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_external_thread_runtime_files_are_runtime_owned(&fs, thread)?;
    fs.create_staged_external_thread_request(
            "qq-turn.tmp",
            "{\"messages\":[{\"role\":\"user\",\"content\":\"群里问题\",\"subject\":\"qq:user:123456\",\"display_name\":\"Alice\"}]}\n",
        )?;
    fs.submit_external_thread_request("qq-turn.tmp", "qq-turn.req.json")?;
    assert_eq!(fs.node_content(quota)?, "1\n");
    {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(
            runtime
                .lookup_child(thread, "state")
                .and_then(crate::Node::content),
            Some("queued\n")
        );
        assert_eq!(
            runtime
                .lookup_child(thread, "messages.jsonl")
                .and_then(crate::Node::content),
            Some(crate::EMPTY_TEXT),
            "external thread submit queues work until control/drain"
        );
        drop(runtime);
    }

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let messages = runtime
        .lookup_child(thread, "messages.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(messages.contains("\"subject\":\"qq:user:123456\""));
    assert!(messages.contains("\"display_name\":\"Alice\""));
    assert!(messages.contains("\"content\":\"群里问题\""));
    assert!(messages.contains("\"content\":\"cortexfs-ok\""));
    assert_eq!(
        runtime
            .lookup_child(thread, "latest.md")
            .and_then(crate::Node::content),
        Some("cortexfs-ok\n")
    );
    assert_eq!(
        runtime
            .lookup_child(thread, "state")
            .and_then(crate::Node::content),
        Some("idle\n")
    );
    assert_eq!(
        runtime.node(quota).and_then(crate::Node::content),
        Some("1\n")
    );
    let episodic = fs.path_inode(["home", "1000", "memory", "episodic"])?;
    let episodic_items = runtime
        .lookup_child(episodic, "items.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(episodic_items.contains("thread=ext/qq/group/888888/thread/demo"));
    assert!(episodic_items.contains("subject=qq:user:123456"));
    assert!(episodic_items.contains("display_name=Alice"));
    assert!(episodic_items.contains("群里问题"));
    drop(runtime);

    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"qq-turn\""));
    assert!(
        export.contains("\"source\":\"ext/qq/group/888888/thread/demo/inbox/qq-turn.req.json\"")
    );
    assert!(export.contains("qq:user:123456"));
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    let queued = audit
        .lines()
        .find(|line| line.contains("\"name\":\"qq-turn.req.json\""))
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(queued.contains("\"external_subject\":\"qq:user:123456\""));
    assert!(queued.contains("\"space\":\"ext/qq/group/888888\""));
    let drained = audit
        .lines()
        .find(|line| {
            line.contains("\"name\":\"qq-turn\"") && line.contains("\"event\":\"drained\"")
        })
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(drained.contains("\"external_subject\":\"qq:user:123456\""));
    assert!(drained.contains("\"space\":\"ext/qq/group/888888\""));
    let filters = fs
        .tree
        .path_inode(crate::EXPORT_FILTERS_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let subject = runtime
            .lookup_child(filters, "subject")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let space = runtime
            .lookup_child(filters, "space")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(subject, 0, b"qq:user:123456\n")?;
        runtime.write(space, 0, b"ext/qq/group/888888\n")?;
        drop(runtime);
    }
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"qq-turn\""));
    assert!(export.contains("qq:user:123456"));
    Ok(())
}

#[test]
fn external_thread_rejects_untrusted_subject() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    assert!(
        fs.tree
            .path_inode(crate::EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH)
            .is_none(),
        "external subject quota requests must be runtime-owned, not a static placeholder"
    );
    let quota = fs.resolve_path_inode(crate::EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH)?;
    fs.create_staged_external_thread_request(
            "qq-bad.tmp",
            "{\"messages\":[{\"role\":\"user\",\"content\":\"spoof\",\"subject\":\"qq:user:999999\",\"display_name\":\"Mallory\"}]}\n",
        )?;
    assert_eq!(
        fs.submit_external_thread_request("qq-bad.tmp", "qq-bad.req.json"),
        Err(fuse3::Errno::from(libc::EACCES))
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert_eq!(fs.node_content(quota)?, "0\n");
    Ok(())
}
