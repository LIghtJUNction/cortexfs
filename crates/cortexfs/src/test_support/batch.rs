use super::support::set_default_provider;
use crate::CortexFs;

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

#[test]
fn batch_rename_queues_request_without_provider_execution() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    assert_batch_runtime_files_are_runtime_owned(&fs)?;
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
fn control_drain_materializes_batch_provider_error() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::local_execution_provider_spec()?;
    set_default_provider(&fs, &provider)?;
    fs.create_staged_batch_request("batch-error.tmp", "{\"messages\":[]}\n")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.plane = Some(cortexd::ExecutionPlane::new(
            cortex_store::InMemoryStore::new(),
            Box::new(cortex_providers::InMemoryProvider::new(
                cortex_core::ProviderId::new(provider.id).map_err(|_error| libc::EIO)?,
                vec![cortex_core::ApiFormat::OpenAiResponses],
            )),
        ));
    }
    fs.submit_batch_request("batch-error.tmp", "batch-error.req.json")?;

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
    let (error, has_response, state, count) = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let error = runtime
            .lookup_child(outbox, "batch-error.error")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let has_response = runtime
            .lookup_child(outbox, "batch-error.resp.json")
            .is_some();
        let state = runtime
            .lookup_child(batch, "state")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned);
        let count = runtime
            .lookup_child(batch, "count")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned);
        drop(runtime);
        (error, has_response, state, count)
    };
    assert!(error.contains("\"request_id\":\"batch-error\""));
    assert!(error.contains("unsupported provider format"));
    assert!(
        !has_response,
        "provider errors must not create success files"
    );
    assert_eq!(state.as_deref(), Some("idle\n"));
    assert_eq!(count.as_deref(), Some("1\n"));
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("last_drained")?)?,
        "batch-error\n"
    );
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(!export.contains("\"request_id\":\"batch-error\""));
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
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
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
