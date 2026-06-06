use crate::CortexFs;

use super::support::set_default_provider;

#[test]
fn export_refresh_derives_sft_from_thread_messages() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_thread_request(
        "export-001.tmp",
        "{\"messages\":[{\"role\":\"user\",\"content\":\"train me\"}]}\n",
    )?;
    fs.submit_thread_request("export-001.tmp", "export-001.req.json")?;
    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let refresh = fs.export_file_inode("refresh")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(refresh, 0, b"1\n")?;
    }

    let sft = fs.node_content(fs.export_file_inode("sft.jsonl")?)?;
    assert!(sft.contains("\"source\":\"thread/demo/messages.jsonl\""));
    assert!(sft.contains("\"role\":\"user\",\"content\":\"train me\""));
    assert!(sft.contains("\"role\":\"assistant\",\"content\":\"cortexfs-ok\""));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"exports\"")
    );
    Ok(())
}

#[test]
fn export_compat_path_exposes_runtime_files() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let primary = fs.export_file_inode("conversations.jsonl")?;
    let compat_parent = fs.path_inode(["home", "1000", "exports"])?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let compat = runtime
        .lookup_child(compat_parent, "conversations.jsonl")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    drop(runtime);

    assert_eq!(
        primary, compat,
        "export compatibility path must expose the same runtime inode"
    );
    Ok(())
}

#[test]
fn preference_feedback_submit_exports_training_pair() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_preference_pair(
            "pref-001.tmp",
            "{\"prompt\":\"pick one\",\"chosen\":{\"role\":\"assistant\",\"content\":\"better\"},\"rejected\":{\"role\":\"assistant\",\"content\":\"worse\"},\"subject\":\"qq:user:123456\"}\n",
        )?;
    fs.submit_preference_pair("pref-001.tmp", "pref-001.req.json")?;
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );

    let preference = fs.export_file_inode("preference.jsonl")?;
    assert!(
        fs.node_content(preference)?.is_empty(),
        "preference feedback queues work until control/drain"
    );

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let export = fs.node_content(preference)?;
    assert!(export.contains("\"source\":\"feedback/preference\""));
    assert!(export.contains("\"request_id\":\"pref-001\""));
    assert!(export.contains("\"chosen\""));
    assert!(export.contains("\"rejected\""));
    assert!(export.contains("qq:user:123456"));

    let outbox = fs
        .tree
        .path_inode(crate::FEEDBACK_PREFERENCE_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let ack = runtime
        .lookup_child(outbox, "pref-001.resp.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(ack.contains("\"status\":\"exported\""));
    drop(runtime);

    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"feedback.preference\"")
    );
    Ok(())
}

#[test]
fn invalid_preference_feedback_materializes_error() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_preference_pair(
        "pref-bad.tmp",
        "{\"chosen\":{\"content\":\"same\"},\"rejected\":{\"content\":\"same\"}}\n",
    )?;
    fs.submit_preference_pair("pref-bad.tmp", "pref-bad.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    assert!(
        fs.node_content(fs.export_file_inode("preference.jsonl")?)?
            .is_empty(),
        "invalid preference pairs must not enter training export"
    );
    let outbox = fs
        .tree
        .path_inode(crate::FEEDBACK_PREFERENCE_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let error = runtime
        .lookup_child(outbox, "pref-bad.error")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(error.contains("chosen and rejected must differ"));
    drop(runtime);
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"event\":\"error\""));
    assert!(audit.contains("\"fingerprint\":\"fnv1a64:"));
    Ok(())
}

#[test]
fn export_refresh_rejects_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let refresh = fs.export_file_inode("refresh")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime.write(refresh, 0, b"yes\n").is_err(),
        "export refresh accepts only 1"
    );
    assert!(
        runtime.write(refresh, 1, b"1\n").is_err(),
        "export refresh requires offset zero"
    );
    drop(runtime);
    Ok(())
}

#[test]
fn conversation_export_uses_submission_time_route() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::local_execution_provider_spec()?;
    let alternate = crate::alternate_provider_spec(&provider)?;
    set_default_provider(&fs, &provider)?;
    fs.create_staged_request("openai.chat", "routed.tmp", "{\"messages\":[]}\n")?;
    fs.submit_request("openai.chat", "routed.tmp", "routed.req.json")?;

    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let default_provider = runtime
            .lookup_child(routes, "default_provider")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(
            default_provider,
            0,
            format!("{}\n", alternate.id).as_bytes(),
        )?;
        drop(runtime);
    }

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"routed\""));
    assert!(export.contains("\"time\":\"00000000000000000001\""));
    assert!(export.contains(&format!("\"route\":{{\"provider\":\"{}\"", provider.id)));
    assert!(!export.contains(&format!("\"route\":{{\"provider\":\"{}\"", alternate.id)));
    Ok(())
}

#[test]
fn daemon_request_store_uses_submission_time_route_provider() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::providers_for_format("openai.responses")
        .find(|provider| provider.id != crate::default_provider_id())
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    set_default_provider(&fs, provider)?;
    fs.create_staged_request(
        "openai.responses",
        "responses.tmp",
        "{\"input\":\"hello\"}\n",
    )?;
    fs.submit_request("openai.responses", "responses.tmp", "responses.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(&[
            "spaces",
            "users",
            "1000",
            "api",
            "openai.responses",
            "outbox",
        ])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let response = runtime
        .lookup_child(outbox, "responses.resp.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(response.contains(&format!("\"provider\":\"{}\"", provider.id)));
    let route = runtime
        .lookup_child(outbox, "responses.route.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(route.contains(&format!("\"provider\":\"{}\"", provider.id)));
    drop(runtime);
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains(&format!("\"route\":{{\"provider\":\"{}\"", provider.id)));
    Ok(())
}

#[test]
fn export_filters_rebuild_conversation_view_by_provider() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let primary = crate::local_execution_provider_spec()?;
    let alternate = crate::alternate_provider_spec(&primary)?;
    set_default_provider(&fs, &primary)?;
    fs.create_staged_request("openai.chat", "primary.tmp", "{\"messages\":[]}\n")?;
    fs.submit_request("openai.chat", "primary.tmp", "primary.req.json")?;
    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let default_provider = runtime
            .lookup_child(routes, "default_provider")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(
            default_provider,
            0,
            format!("{}\n", alternate.id).as_bytes(),
        )?;
        drop(runtime);
    }
    fs.create_staged_request("openai.chat", "alternate.tmp", "{\"messages\":[]}\n")?;
    fs.submit_request("openai.chat", "alternate.tmp", "alternate.req.json")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let filters = fs
        .tree
        .path_inode(&["spaces", "users", "1000", "export", "filters"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let provider = runtime
            .lookup_child(filters, "provider")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(provider, 0, format!("{}\n", alternate.id).as_bytes())?;
        drop(runtime);
    }

    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"alternate\""));
    assert!(!export.contains("\"request_id\":\"primary\""));
    assert!(export.contains(&format!("\"provider\":\"{}\"", alternate.id)));

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let agent = runtime
            .lookup_child(filters, "agent")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let space = runtime
            .lookup_child(filters, "space")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(agent, 0, b"helper\n")?;
        runtime.write(space, 0, b"home/1000\n")?;
        drop(runtime);
    }
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"alternate\""));

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let subject = runtime
            .lookup_child(filters, "subject")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(subject, 0, b"qq:user:123456\n")?;
        drop(runtime);
    }
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(
        export.is_empty(),
        "subject filter must exclude rows without a subject"
    );
    Ok(())
}

#[test]
fn export_filters_rebuild_conversation_view_by_time_range() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_request("openai.chat", "first.tmp", "{\"messages\":[]}\n")?;
    fs.submit_request("openai.chat", "first.tmp", "first.req.json")?;
    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    fs.create_staged_request("openai.chat", "second.tmp", "{\"messages\":[]}\n")?;
    fs.submit_request("openai.chat", "second.tmp", "second.req.json")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"first\""));
    assert!(export.contains("\"time\":\"00000000000000000001\""));
    assert!(export.contains("\"request_id\":\"second\""));
    assert!(export.contains("\"time\":\"00000000000000000002\""));

    let filters = fs
        .tree
        .path_inode(crate::EXPORT_FILTERS_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let from = runtime
            .lookup_child(filters, "from")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(from, 0, b"2\n")?;
        drop(runtime);
    }
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(!export.contains("\"request_id\":\"first\""));
    assert!(export.contains("\"request_id\":\"second\""));

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let from = runtime
            .lookup_child(filters, "from")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let to = runtime
            .lookup_child(filters, "to")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(from, 0, b"\n")?;
        runtime.write(to, 0, b"1\n")?;
        drop(runtime);
    }
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"first\""));
    assert!(!export.contains("\"request_id\":\"second\""));
    Ok(())
}

#[test]
fn export_filters_reject_invalid_values() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let filters = fs
        .tree
        .path_inode(&["spaces", "users", "1000", "export", "filters"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let provider = runtime
        .lookup_child(filters, "provider")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let exclude_failed = runtime
        .lookup_child(filters, "exclude_failed")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let agent = runtime
        .lookup_child(filters, "agent")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let from = runtime
        .lookup_child(filters, "from")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    assert_eq!(
        runtime.write(
            provider,
            0,
            format!("{}\n", crate::invalid_provider_id()).as_bytes()
        ),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    assert_eq!(
        runtime.write(exclude_failed, 0, b"true\n"),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    assert_eq!(
        runtime.write(agent, 1, b"helper\n"),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    assert_eq!(
        runtime.write(from, 0, b"not-a-time\n"),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    let valid_provider = crate::default_provider_spec()?;
    assert_eq!(
        runtime.write(provider, 1, format!("{}\n", valid_provider.id).as_bytes()),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    drop(runtime);
    Ok(())
}
