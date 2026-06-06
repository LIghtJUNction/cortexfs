use crate::CortexFs;

#[test]
fn batch_rename_queues_request_without_provider_execution() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_batch_request("batch-001.tmp", "{\"messages\":[]}\n")?;

    fs.submit_batch_request("batch-001.tmp", "batch-001.req.json")?;

    let outbox = fs
        .tree
        .path_inode(crate::BATCH_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        fs.runtime
            .lock()
            .map_err(|_error| libc::EIO)?
            .lookup_child(outbox, "batch-001.resp.json")
            .is_none(),
        "batch rename must not execute the provider inside the FUSE callback"
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );
    let batch = fs
        .tree
        .path_inode(crate::BATCH_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let (state, count, audit) = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let state = runtime
            .lookup_child(batch, "state")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned);
        let count = runtime
            .lookup_child(batch, "count")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned);
        let audit = runtime
            .node(runtime.audit_inode)
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned)
            .ok_or(libc::EISDIR)?;
        drop(runtime);
        (state, count, audit)
    };
    assert_eq!(state.as_deref(), Some("queued\n"));
    assert_eq!(count.as_deref(), Some("1\n"));
    assert!(audit.contains("\"name\":\"batch-001.req.json\""));
    Ok(())
}

#[test]
fn control_drain_materializes_batch_response() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_batch_request("batch-001.tmp", "{\"messages\":[]}\n")?;
    fs.submit_batch_request("batch-001.tmp", "batch-001.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(crate::BATCH_OUTBOX_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let batch = fs
        .tree
        .path_inode(crate::BATCH_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let (response, state, count, audit) = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let response = runtime
            .lookup_child(outbox, "batch-001.resp.json")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let state = runtime
            .lookup_child(batch, "state")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned);
        let count = runtime
            .lookup_child(batch, "count")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned);
        let audit = runtime
            .node(runtime.audit_inode)
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned)
            .ok_or(libc::EISDIR)?;
        drop(runtime);
        (response, state, count, audit)
    };
    assert!(response.contains("\"content\":\"cortexfs-ok\""));
    assert_eq!(state.as_deref(), Some("idle\n"));
    assert_eq!(count.as_deref(), Some("1\n"));
    assert!(audit.contains("\"event\":\"drained\""));
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"batch-001\""));
    assert!(export.contains("\"format\":\"openai.chat\""));
    assert!(export.contains("\"fingerprint\":\"fnv1a64:"));
    assert!(export.contains("cortexfs-ok"));
    Ok(())
}

#[test]
fn invalid_submit_suffix_is_rejected() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_request("openai.chat", "bad.tmp", "{}\n")?;

    assert!(
        fs.submit_request("openai.chat", "bad.tmp", "bad.json")
            .is_err(),
        "only *.req.json commits a request"
    );
    Ok(())
}

#[test]
fn duplicate_request_id_reuses_existing_outbox() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_request("openai.chat", "first.tmp", "{\"messages\":[]}\n")?;
    fs.submit_request("openai.chat", "first.tmp", "same.req.json")?;
    {
        let drain = fs.control_file_inode("drain")?;
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(&["spaces", "users", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let first_fingerprint = fs
        .runtime
        .lock()
        .map_err(|_error| libc::EIO)?
        .lookup_child(outbox, "same.fingerprint")
        .and_then(crate::Node::content)
        .map(ToOwned::to_owned)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    fs.create_staged_request("openai.chat", "second.tmp", "{\"messages\":[1]}\n")?;
    fs.submit_request("openai.chat", "second.tmp", "same.req.json")?;

    let (second_fingerprint, has_response, has_duplicate_audit) = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let fingerprint = runtime
            .lookup_child(outbox, "same.fingerprint")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let has_response = runtime.lookup_child(outbox, "same.resp.json").is_some();
        let has_duplicate_audit = runtime
            .node(runtime.audit_inode)
            .and_then(crate::Node::content)
            .ok_or(libc::EISDIR)?
            .contains("\"event\":\"duplicate\"");
        drop(runtime);
        (fingerprint, has_response, has_duplicate_audit)
    };
    assert_eq!(second_fingerprint, first_fingerprint);
    assert!(has_response, "existing response remains available");
    assert!(has_duplicate_audit);
    Ok(())
}
