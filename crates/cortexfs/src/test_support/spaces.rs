use crate::CortexFs;
use fuse3::FileType;

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
            fs.lookup_path(["spaces", "users", "1000", "api", format, "inbox"])
                .is_some(),
            "format inbox must exist"
        );
        assert!(
            fs.lookup_path(["spaces", "users", "1000", "api", format, "outbox"])
                .is_some(),
            "format outbox must exist"
        );
    }
}

#[test]
fn projection_exposes_home_uid_alias_for_user_space() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let canonical = fs
        .tree
        .path_inode(&["spaces", "users", "1000"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let home = fs
        .tree
        .path_inode(&["home", "1000"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    assert_eq!(home, canonical);
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
        fs.lookup_path(["home", "1000", "threads", "demo"])
            .is_some(),
        "home/<uid> must expose the user's thread namespace"
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "tools", "list"])
            .and_then(crate::Node::content),
        Some("filesystem.read\nmcp.local-fs.read_file\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "mcp", "servers", "list"])
            .and_then(crate::Node::content),
        Some("local-fs\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "skills", "enabled"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert!(
        fs.lookup_path(["home", "1000", "memory", "semantic"])
            .is_some(),
        "home/<uid> must expose the user's memory layers"
    );
    assert!(
        fs.lookup_path(["home", "1000", "exports", "formats"])
            .and_then(crate::Node::content)
            .is_some_and(|formats| formats.contains("conversations.jsonl")
                && formats.contains("tool_calls.jsonl")),
        "home/<uid> must expose training-friendly export formats"
    );
    Ok(())
}

#[test]
fn projection_exposes_space_capability_and_maintenance_indexes() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "agents", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "agents", "list"])
            .and_then(crate::Node::content),
        Some("helper\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "tools", "count"])
            .and_then(crate::Node::content),
        Some("2\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "tools", "list"])
            .and_then(crate::Node::content),
        Some("filesystem.read\nmcp.local-fs.read_file\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "mcp", "servers", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "mcp", "tools", "list"])
            .and_then(crate::Node::content),
        Some("local-fs.read_file\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "skills", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "skills", "list"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "cache", "status"])
            .and_then(crate::Node::content),
        Some("enabled\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "cache", "entries"])
            .and_then(crate::Node::content),
        Some("0\n")
    );
    assert!(
        fs.lookup_path(["spaces", "users", "1000", "cache", "keys"])
            .is_some(),
        "space cache keys namespace must exist"
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "audit", "status"])
            .and_then(crate::Node::content),
        Some("enabled\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "audit", "scope"])
            .and_then(crate::Node::content),
        Some("space\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "convert", "formats"])
            .and_then(crate::Node::content),
        Some("sft.jsonl\npreference.jsonl\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "convert", "status"])
            .and_then(crate::Node::content),
        Some("idle\n")
    );
}

#[test]
fn projection_exposes_space_exports_batch_feedback_and_control() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["spaces", "users", "1000", "exports"])
            .is_some(),
        "exports directory must exist"
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "exports", "formats"])
            .and_then(crate::Node::content),
        Some(
            "conversations.jsonl\nsft.jsonl\npreference.jsonl\ntool_calls.jsonl\nagent_traces.jsonl\n"
        )
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "exports", "dedupe"])
            .and_then(crate::Node::content),
        Some("fingerprint\n")
    );
    assert!(
        fs.lookup_path(["spaces", "users", "1000", "exports", "sources"])
            .and_then(crate::Node::content)
            .is_some_and(|sources| sources.contains("threads/*/messages.jsonl")
                && sources.contains("audit/events.jsonl")
                && sources.contains("human feedback")),
        "export sources must document provenance"
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "exports", "redaction"])
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
        "exclude_failed",
    ] {
        assert!(runtime.lookup_child(filters, filter_file).is_some());
    }
    drop(runtime);
    assert!(
        fs.lookup_path(["spaces", "users", "1000", "batch", "inbox"])
            .is_some(),
        "batch inbox must exist"
    );
    assert!(
        fs.lookup_path(["spaces", "users", "1000", "batch", "outbox"])
            .is_some(),
        "batch outbox must exist"
    );
    assert!(
        fs.lookup_path(["spaces", "users", "1000", "feedback", "preference", "inbox"])
            .is_some(),
        "preference feedback inbox must exist"
    );
    assert!(
        fs.lookup_path([
            "spaces",
            "users",
            "1000",
            "feedback",
            "preference",
            "outbox"
        ])
        .is_some(),
        "preference feedback outbox must exist"
    );
    assert!(
        fs.lookup_path(["spaces", "users", "1000", "control", "reload"])
            .is_some(),
        "space reload control node must exist"
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
            "spaces",
            "external",
            "qq",
            "groups",
            "888888",
            "subjects",
            "123456",
            "display_name"
        ])
        .and_then(crate::Node::content),
        Some("Alice\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "external", "qq", "groups", "888888", "context"])
            .and_then(crate::Node::content),
        Some("qq:group888888:object_r:group_thread_t:s0:c_qq,c_group888888\n")
    );
    assert!(
        fs.lookup_path([
            "spaces", "external", "qq", "groups", "888888", "threads", "demo", "inbox"
        ])
        .is_some(),
        "external group thread inbox must exist"
    );
    assert_eq!(
        fs.lookup_path([
            "spaces", "external", "qq", "groups", "888888", "threads", "demo", "io.sock"
        ])
        .map(crate::Node::kind),
        Some(FileType::Socket)
    );
    let external_socket = fs
        .tree
        .path_inode(&[
            "spaces", "external", "qq", "groups", "888888", "threads", "demo", "io.sock",
        ])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        fs.node_content(external_socket).is_err(),
        "external group thread socket is a realtime endpoint, not a regular file"
    );
    assert_eq!(
        fs.node_content(
            fs.tree
                .path_inode(crate::EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH)
                .ok_or_else(fuse3::Errno::new_not_exist)?
        )?,
        "0\n"
    );
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
