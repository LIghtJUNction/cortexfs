use crate::CortexFs;
use fuse3::FileType;

fn assert_batch_runtime_files_are_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let batch = fs
        .tree
        .path_inode(crate::BATCH_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let entries = fs.children(batch);
    for name in ["state", "count"] {
        assert!(
            fs.tree
                .path_inode(&["home", "1000", "batch", name])
                .is_none(),
            "batch {name} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "batch directory must expose one {name} entry"
        );
    }
    Ok(())
}

fn assert_convert_status_is_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let convert = fs
        .tree
        .path_inode(&["home", "1000", "convert"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let entries = fs.children(convert);
    assert!(
        fs.tree
            .path_inode(&["home", "1000", "convert", "status"])
            .is_none(),
        "convert status must be runtime-owned, not a static placeholder"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.name.to_str() == Some("status"))
            .count(),
        1,
        "convert directory must expose one status entry"
    );
    assert_eq!(
        fs.node_content(fs.resolve_path_inode(["home", "1000", "convert", "status"])?)?,
        "idle\n"
    );
    Ok(())
}

fn assert_cache_runtime_files_are_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let cache = fs
        .tree
        .path_inode(&["home", "1000", "cache"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let entries = fs.children(cache);
    for (name, expected) in [("status", "enabled\n"), ("entries", "0\n")] {
        assert!(
            fs.tree
                .path_inode(&["home", "1000", "cache", name])
                .is_none(),
            "cache {name} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "cache directory must expose one {name} entry"
        );
        assert_eq!(
            fs.node_content(fs.resolve_path_inode(["home", "1000", "cache", name])?)?,
            expected
        );
    }
    Ok(())
}

fn assert_space_audit_runtime_files_are_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let audit = fs
        .tree
        .path_inode(&["home", "1000", "audit"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let entries = fs.children(audit);
    for (name, expected) in [("status", "enabled\n"), ("events", "0\n")] {
        assert!(
            fs.tree
                .path_inode(&["home", "1000", "audit", name])
                .is_none(),
            "space audit {name} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "space audit directory must expose one {name} entry"
        );
        assert_eq!(
            fs.node_content(fs.resolve_path_inode(["home", "1000", "audit", name])?)?,
            expected
        );
    }
    Ok(())
}

#[test]
fn projection_exposes_space_api_inbox_outbox_for_each_format() {
    let fs = CortexFs::new();

    for format in [
        "openai.chat",
        "openai.responses",
        "anthropic.messages",
        "google.generate_content",
    ] {
        assert!(
            fs.lookup_path(["home", "1000", "api", format, "inbox"])
                .is_some(),
            "format inbox must exist"
        );
        assert!(
            fs.lookup_path(["home", "1000", "api", format, "outbox"])
                .is_some(),
            "format outbox must exist"
        );
    }
}

#[test]
fn projection_exposes_home_uid_as_the_only_user_space_entry() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let canonical = fs
        .tree
        .path_inode(&["home", "1000"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    assert!(fs.tree.path_inode(&["space", "users", "1000"]).is_none());
    assert_ne!(fs.tree.path_inode(&["space", "uid1000"]), Some(canonical));
    assert!(fs.lookup_path(["spaces"]).is_none());
    assert!(fs.lookup_path(["ctx_home"]).is_none());
    assert!(fs.lookup_path(["home", "count"]).is_none());
    assert!(fs.lookup_path(["home", "list"]).is_none());
    assert!(fs.lookup_path(["home", "current"]).is_none());
    assert_eq!(
        fs.lookup_path(["home", "1000", "uid"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_USER_UID_TEXT)
    );
    assert!(fs.lookup_path(["home", "1000", "home"]).is_none());
    assert!(fs.lookup_path(["home", "1000", "space"]).is_none());
    assert!(
        fs.lookup_path(["home", "1000", "api", "openai.chat", "inbox"])
            .is_some(),
        "home/<uid> must expose the user's API inbox"
    );
    assert!(
        fs.lookup_path(["home", "1000", "thread", "demo"]).is_some(),
        "home/<uid> must expose the user's thread namespace"
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "tool", "list"])
            .and_then(crate::Node::content),
        Some("filesystem.read\nmcp.local-fs.read_file\n")
    );
    assert!(fs.lookup_path(["home", "1000", "tool", "list"]).is_some());
    assert_eq!(
        fs.lookup_path(["home", "1000", "mcp", "server", "list"])
            .and_then(crate::Node::content),
        Some("local-fs\n")
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "server", "list"])
            .is_some()
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "skill", "enabled"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert!(
        fs.lookup_path(["home", "1000", "skill", "enabled"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["home", "1000", "memory", "semantic"])
            .is_some(),
        "home/<uid> must expose the user's memory layer directories"
    );
    assert!(
        fs.lookup_path(["home", "1000", "export", "format"])
            .and_then(crate::Node::content)
            .is_some_and(|format| format.contains("conversations.jsonl")
                && format.contains("tool_calls.jsonl")),
        "home/<uid> must expose training-friendly export format"
    );
    assert!(
        fs.lookup_path(["home", "1000", "export", "formats"])
            .is_none()
    );
    Ok(())
}

#[test]
fn audit_events_include_required_context_fields() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_tool_request("audit-read.tmp", "{\"path\":\"status\"}\n")?;
    fs.submit_tool_request("audit-read.tmp", "audit-read.req.json")?;
    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let audit = fs.node_content(fs.audit_events_inode()?)?;
    let space_audit_events =
        fs.node_content(fs.resolve_path_inode(["home", "1000", "audit", "events"])?)?;
    assert_eq!(space_audit_events, format!("{}\n", audit.lines().count()));
    assert_ne!(space_audit_events, "0\n");
    let drained = audit
        .lines()
        .find(|line| line.contains("\"event\":\"drained\"") && line.contains("filesystem.read"))
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    for field in [
        "\"host_uid\":1000",
        "\"host_gid\":1000",
        "\"host_pid\":0",
        "\"external_subject\":null",
        "\"space\":\"home/1000\"",
        "\"agent\":\"helper\"",
        "\"operation\":\"invoke\"",
        "\"object_class\":\"mcp_tool\"",
        "\"tool\":\"filesystem.read\"",
        "\"decision\":\"allow\"",
        "\"latency_ms\":0",
        "\"input_tok\":0",
        "\"output_tok\":0",
        "\"cost_usd\":0",
        "\"error\":null",
        "\"fingerprint\":",
    ] {
        assert!(drained.contains(field), "audit event missing field {field}");
    }
    Ok(())
}

#[test]
fn projection_exposes_space_as_read_only_context_index() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["space", "list"])
            .and_then(crate::Node::content),
        Some("uid1000\nshared.project-a\next.qq\n")
    );
    assert_eq!(
        fs.lookup_path(["space", "uid1000", "entry"])
            .and_then(crate::Node::content),
        Some("home/1000\n")
    );
    assert_eq!(
        fs.lookup_path(["space", "shared.project-a", "entry"])
            .and_then(crate::Node::content),
        Some("shared/project-a\n")
    );
    assert_eq!(
        fs.lookup_path(["space", "ext.qq", "entry"])
            .and_then(crate::Node::content),
        Some("ext/qq\n")
    );
    assert!(
        fs.lookup_path(["space", "uid1000", "api"]).is_none(),
        "space index must not duplicate the home API entry"
    );
    let space_uid = fs
        .tree
        .path_inode(&["space", "uid1000"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let home_uid = fs
        .tree
        .path_inode(&["home", "1000"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_ne!(space_uid, home_uid);

    assert!(
        fs.tree
            .path_inode(&["spaces", "shared", "project-a"])
            .is_none()
    );
    assert!(fs.tree.path_inode(&["spaces", "external", "qq"]).is_none());
    assert!(
        fs.tree
            .path_inode(&["space", "shared", "project-a"])
            .is_none()
    );
    assert!(fs.tree.path_inode(&["space", "external", "qq"]).is_none());
    Ok(())
}

#[test]
fn projection_exposes_space_capability_and_maintenance_indexes() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["home", "1000", "agent", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "agent", "list"])
            .and_then(crate::Node::content),
        Some("helper\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "tool", "count"])
            .and_then(crate::Node::content),
        Some("2\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "tool", "list"])
            .and_then(crate::Node::content),
        Some("filesystem.read\nmcp.local-fs.read_file\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "mcp", "server", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "mcp", "tool", "list"])
            .and_then(crate::Node::content),
        Some("local-fs.read_file\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "skill", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "skill", "list"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert_cache_runtime_files_are_runtime_owned(&fs)?;
    assert!(
        fs.lookup_path(["home", "1000", "cache", "keys"]).is_some(),
        "space cache keys namespace must exist"
    );
    assert_space_audit_runtime_files_are_runtime_owned(&fs)?;
    assert_eq!(
        fs.lookup_path(["home", "1000", "audit", "scope"])
            .and_then(crate::Node::content),
        Some("space\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "convert", "format"])
            .and_then(crate::Node::content),
        Some("sft.jsonl\npreference.jsonl\n")
    );
    assert!(
        fs.lookup_path(["home", "1000", "convert", "formats"])
            .is_none()
    );
    assert_convert_status_is_runtime_owned(&fs)?;
    Ok(())
}

#[test]
fn projection_exposes_space_exports_batch_feedback_and_control() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["home", "1000", "export"]).is_some(),
        "export directory must exist"
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "export", "format"])
            .and_then(crate::Node::content),
        Some(
            "conversations.jsonl\nsft.jsonl\npreference.jsonl\ntool_calls.jsonl\nagent_traces.jsonl\n"
        )
    );
    assert!(
        fs.lookup_path(["home", "1000", "export", "formats"])
            .is_none()
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "export", "dedupe"])
            .and_then(crate::Node::content),
        Some("fingerprint\n")
    );
    assert!(
        fs.lookup_path(["home", "1000", "export", "source"])
            .and_then(crate::Node::content)
            .is_some_and(|source| source.contains("home/*/thread/*/messages.jsonl")
                && source.contains("home/*/audit/events.jsonl")
                && source.contains("tool/*/invoke/inbox/*.req.json")
                && source.contains("home/*/feedback/preference/inbox/*.req.json")),
        "export source must document provenance"
    );
    assert!(
        fs.lookup_path(["home", "1000", "export", "sources"])
            .is_none()
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "export", "redaction"])
            .and_then(crate::Node::content),
        Some("policy\n")
    );
    assert_eq!(
        fs.node_content(fs.export_file_inode("conversations.jsonl")?)?,
        crate::EMPTY_TEXT
    );
    for export_file in [
        "sft.jsonl",
        "preference.jsonl",
        "tool_calls.jsonl",
        "agent_traces.jsonl",
        "refresh",
    ] {
        assert!(
            fs.export_file_inode(export_file).is_ok(),
            "export file must exist: {export_file}"
        );
    }
    let filters = fs
        .tree
        .path_inode(crate::EXPORT_FILTERS_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    for filter_file in [
        "provider",
        "model",
        "agent",
        "subject",
        "space",
        "from",
        "to",
        "exclude_failed",
    ] {
        assert!(runtime.lookup_child(filters, filter_file).is_some());
    }
    drop(runtime);
    assert!(
        fs.lookup_path(["home", "1000", "batch", "inbox"]).is_some(),
        "batch inbox must exist"
    );
    assert!(
        fs.lookup_path(["home", "1000", "batch", "outbox"])
            .is_some(),
        "batch outbox must exist"
    );
    assert_batch_runtime_files_are_runtime_owned(&fs)?;
    assert!(
        fs.lookup_path(["home", "1000", "feedback", "preference", "inbox"])
            .is_some(),
        "preference feedback inbox must exist"
    );
    assert!(
        fs.lookup_path(["home", "1000", "feedback", "preference", "outbox"])
            .is_some(),
        "preference feedback outbox must exist"
    );
    assert!(
        fs.lookup_path(["home", "1000", "control", "reload"])
            .is_none(),
        "space reload control node must not exist during development"
    );
    let batch = fs
        .tree
        .path_inode(crate::BATCH_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let (state, count) = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let state = runtime
            .lookup_child(batch, "state")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned);
        let count = runtime
            .lookup_child(batch, "count")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned);
        drop(runtime);
        (state, count)
    };
    assert_eq!(state.as_deref(), Some("idle\n"));
    assert_eq!(count.as_deref(), Some("0\n"));
    Ok(())
}

#[test]
fn projection_exposes_external_subject_and_audit_summary_shape() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path([
            "ext",
            "qq",
            "group",
            "888888",
            "subject",
            "123456",
            "display_name"
        ])
        .and_then(crate::Node::content),
        Some("Alice\n")
    );
    let direct_subject = fs
        .tree
        .path_inode(&["ext", "qq", "group", "888888", "subject", "123456"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(fs.node_attr(direct_subject)?.kind, FileType::Directory);
    assert!(
        fs.tree
            .path_inode(&[
                "home", "external", "qq", "group", "888888", "subject", "123456",
            ])
            .is_none()
    );
    assert_eq!(
        fs.lookup_path(["ext", "qq", "group", "888888", "context"])
            .and_then(crate::Node::content),
        Some("qq:group888888:object_r:group_thread_t:s0:c_qq,c_group888888\n")
    );
    assert!(
        fs.lookup_path(["ext", "qq", "group", "888888", "thread", "demo", "inbox"])
            .is_some(),
        "external group thread inbox must exist"
    );
    assert_eq!(
        fs.lookup_path(["ext", "qq", "group", "888888", "thread", "demo", "io.sock"])
            .map(crate::Node::kind),
        Some(FileType::Socket)
    );
    let external_socket = fs
        .tree
        .path_inode(&["ext", "qq", "group", "888888", "thread", "demo", "io.sock"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        fs.node_content(external_socket).is_err(),
        "external group thread socket is a realtime endpoint, not a regular file"
    );
    assert!(
        fs.tree
            .path_inode(crate::EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH)
            .is_none(),
        "external subject quota requests must be runtime-owned, not a static placeholder"
    );
    let quota = fs.resolve_path_inode(crate::EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH)?;
    assert_eq!(fs.node_content(quota)?, "0\n");
    assert_eq!(
        fs.lookup_path(["audit", "context"])
            .and_then(crate::Node::content),
        Some("local:audit_r:audit_log_t:s0\n")
    );
    let fields = fs
        .lookup_path(["audit", "fields"])
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(fields.contains("host_uid\n"));
    assert!(fields.contains("external_subject\n"));
    assert!(fields.contains("object_class\n"));
    assert!(fields.contains("fingerprint\n"));
    let object_classes = fs
        .lookup_path(["audit", "object_classes"])
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(object_classes.contains("audit_log\n"));
    assert!(object_classes.contains("mcp_tool\n"));
    assert!(object_classes.contains("vector_index\n"));
    let verbs = fs
        .lookup_path(["audit", "verbs"])
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(verbs.contains("submit\n"));
    assert!(verbs.contains("invoke\n"));
    assert!(verbs.contains("relabel\n"));
    assert!(verbs.contains("retrieve\n"));
    assert_eq!(
        fs.lookup_path(["audit", "redaction"])
            .and_then(crate::Node::content),
        Some("secrets=always\nprompts=policy\n")
    );
    assert_eq!(
        fs.node_content(fs.audit_usage_inode()?)?,
        "events=0\nstaged=0\nqueued=0\ndrained=0\nerrors=0\ndenied=0\n"
    );
    assert_eq!(
        fs.node_content(fs.audit_cost_inode()?)?,
        "usd=0.000000\nbillable_events=0\ndrained=0\ntool_calls=0\nagent_tasks=0\n"
    );
    Ok(())
}
