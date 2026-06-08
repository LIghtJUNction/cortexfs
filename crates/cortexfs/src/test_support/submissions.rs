use crate::CortexFs;

use super::support::set_default_provider;

#[test]
fn staged_request_write_does_not_create_outbox_response() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let inbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "inbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let outbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    let inode = fs.create_staged_request("openai.chat", "001.tmp", "{\"messages\":[]}\n")?;

    assert!(
        fs.runtime
            .lock()
            .map_err(|_error| libc::EIO)?
            .lookup_child(outbox, "001.resp.json")
            .is_none(),
        "write must not trigger response creation"
    );
    assert!(
        fs.runtime
            .lock()
            .map_err(|_error| libc::EIO)?
            .lookup_child(inbox, "001.tmp")
            .is_some(),
        "staged request remains visible in inbox"
    );
    assert_eq!(fs.node_content(inode)?, "{\"messages\":[]}\n");
    Ok(())
}

#[test]
fn rename_to_req_json_queues_request_and_writes_fingerprint() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::default_provider_spec()?;
    fs.create_staged_request("openai.chat", "001.tmp", "{\"messages\":[]}\n")?;

    fs.submit_request("openai.chat", "001.tmp", "001.req.json")?;

    let outbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        fs.runtime
            .lock()
            .map_err(|_error| libc::EIO)?
            .lookup_child(outbox, "001.resp.json")
            .is_none(),
        "rename must not execute the provider inside the FUSE callback"
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );
    let fingerprint = fs
        .runtime
        .lock()
        .map_err(|_error| libc::EIO)?
        .lookup_child(outbox, "001.fingerprint")
        .cloned()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let fingerprint_content = fingerprint.content().ok_or(libc::EISDIR)?;
    assert!(
        fingerprint_content.starts_with("fnv1a64:"),
        "fingerprint must expose the request hash"
    );
    let route = fs
        .runtime
        .lock()
        .map_err(|_error| libc::EIO)?
        .lookup_child(outbox, "001.route.json")
        .and_then(crate::Node::content)
        .map(ToOwned::to_owned)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(route.contains("\"format\":\"openai.chat\""));
    assert!(route.contains(&format!("\"provider\":\"{}\"", provider.id)));
    assert!(route.contains(&format!("\"model\":\"{}\"", provider.default_model)));
    assert!(route.contains("\"reason\":\"ready\""));
    assert!(route.contains("\"fingerprint\":\"fnv1a64:"));
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"event\":\"queued\""));
    assert!(audit.contains("\"fingerprint\":\"fnv1a64:"));
    assert!(audit.contains(&format!("\"provider\":\"{}\"", provider.id)));
    assert!(audit.contains(&format!("\"model\":\"{}\"", provider.default_model)));
    assert!(audit.contains("\"decision\":\"ready\""));
    Ok(())
}

#[test]
fn duplicate_pending_request_id_is_rejected_without_rewriting_staged_request() -> fuse3::Result<()>
{
    let fs = CortexFs::new();
    fs.create_staged_request("openai.chat", "same-a.tmp", "{\"messages\":[\"first\"]}\n")?;
    fs.submit_request("openai.chat", "same-a.tmp", "same.req.json")?;
    let outbox = fs.path_inode(["home", "1000", "api", "openai.chat", "outbox"])?;
    let original_fingerprint = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(outbox, "same.fingerprint")
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned)
            .ok_or_else(fuse3::Errno::new_not_exist)?
    };
    fs.create_staged_request("openai.chat", "same-b.tmp", "{\"messages\":[\"second\"]}\n")?;

    assert_eq!(
        fs.submit_request("openai.chat", "same-b.tmp", "same.req.json"),
        Err(fuse3::Errno::from(libc::EAGAIN))
    );

    let inbox = fs.path_inode(["home", "1000", "api", "openai.chat", "inbox"])?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime.lookup_child(inbox, "same.req.json").is_some(),
        "original queued request must remain visible under the committed request id"
    );
    assert!(
        runtime.lookup_child(inbox, "same-b.tmp").is_some(),
        "duplicate request must remain staged under its original name"
    );
    assert_eq!(
        runtime
            .lookup_child(outbox, "same.fingerprint")
            .and_then(crate::Node::content),
        Some(original_fingerprint.as_str()),
        "duplicate submission must not rewrite the original request fingerprint"
    );
    drop(runtime);
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );
    Ok(())
}

#[test]
fn home_uid_api_inbox_queues_request() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let inbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "inbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let submission = fs.api_submission(inbox).ok_or(libc::EINVAL)?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, "openai.chat", "home-uid.tmp")?;
        runtime.write(inode, 0, b"{\"messages\":[]}\n")?;
        runtime.submit(
            inbox,
            "home-uid.tmp",
            inbox,
            "home-uid.req.json",
            submission,
        )?;
    }

    let outbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        fs.runtime
            .lock()
            .map_err(|_error| libc::EIO)?
            .lookup_child(outbox, "home-uid.route.json")
            .is_some(),
        "home/<uid> submissions must use the canonical user outbox"
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );
    Ok(())
}

#[test]
fn api_submit_is_denied_when_current_route_is_not_allowed() -> fuse3::Result<()> {
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

    fs.create_staged_request("openai.chat", "denied.tmp", "{\"messages\":[]}\n")?;
    assert_eq!(
        fs.submit_request("openai.chat", "denied.tmp", "denied.req.json"),
        Err(fuse3::Errno::from(libc::EACCES))
    );

    let inbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "inbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let outbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime.lookup_child(inbox, "denied.tmp").is_some(),
        "denied request must remain staged under its original name"
    );
    assert!(
        runtime.lookup_child(inbox, "denied.req.json").is_none(),
        "denied request must not be committed"
    );
    assert!(
        runtime.lookup_child(outbox, "denied.fingerprint").is_none(),
        "denied request must not materialize a fingerprint"
    );
    drop(runtime);
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"event\":\"denied\""));
    assert!(audit.contains("\"name\":\"denied.req.json\""));
    assert!(audit.contains(&format!("\"provider\":\"{}\"", provider.id)));
    assert!(audit.contains("\"decision\":\"provider_disabled\""));
    Ok(())
}

#[test]
fn api_submit_uses_current_provider_route_for_access() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::alternate_provider_spec(&crate::default_provider_spec()?)?;
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
        runtime.write(default_provider, 0, format!("{}\n", provider.id).as_bytes())?;
        drop(runtime);
    }

    fs.create_staged_request("openai.chat", "openai-route.tmp", "{\"messages\":[]}\n")?;
    fs.submit_request("openai.chat", "openai-route.tmp", "openai-route.req.json")?;

    let outbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let route = fs
        .runtime
        .lock()
        .map_err(|_error| libc::EIO)?
        .lookup_child(outbox, "openai-route.route.json")
        .and_then(crate::Node::content)
        .map(ToOwned::to_owned)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(route.contains(&format!("\"provider\":\"{}\"", provider.id)));
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );
    Ok(())
}

#[test]
fn api_submit_rejects_invalid_model_field() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_request("openai.chat", "bad-model-field.tmp", "{\"model\":123}\n")?;
    assert_eq!(
        fs.submit_request(
            "openai.chat",
            "bad-model-field.tmp",
            "bad-model-field.req.json",
        ),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    Ok(())
}

#[test]
fn control_drain_materializes_queued_response() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::local_execution_provider_spec()?;
    set_default_provider(&fs, &provider)?;
    fs.create_staged_request("openai.chat", "001.tmp", "{\"messages\":[]}\n")?;
    fs.submit_request("openai.chat", "001.tmp", "001.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let response = fs
        .runtime
        .lock()
        .map_err(|_error| libc::EIO)?
        .lookup_child(outbox, "001.resp.json")
        .cloned()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        response
            .content()
            .ok_or(libc::EISDIR)?
            .contains("\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\"")
    );
    assert!(
        response
            .content()
            .ok_or(libc::EISDIR)?
            .contains("\"content\":\"cortexfs-ok\"")
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("last_drained")?)?,
        "001\n"
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"event\":\"drained\""));
    assert!(audit.contains("\"fingerprint\":\"fnv1a64:"));
    assert!(audit.contains(&format!("\"provider\":\"{}\"", provider.id)));
    assert!(audit.contains(&format!("\"model\":\"{}\"", provider.default_model)));
    assert!(audit.contains("\"decision\":\"ready\""));
    let export = fs.node_content(fs.export_file_inode("conversations.jsonl")?)?;
    assert!(export.contains("\"request_id\":\"001\""));
    assert!(export.contains("\"source\":\"home/1000/api/openai.chat/inbox/001.req.json\""));
    assert!(export.contains("\"format\":\"openai.chat\""));
    assert!(export.contains("\"fingerprint\":\"fnv1a64:"));
    assert!(export.contains(&format!("\"route\":{{\"provider\":\"{}\"", provider.id)));
    assert!(export.contains(&format!("\"model\":\"{}\"", provider.default_model)));
    assert!(export.contains("\"reason\":\"ready\""));
    assert!(export.contains("\\\"messages\\\":[]"));
    assert!(export.contains("cortexfs-ok"));
    let usage = fs.node_content(fs.audit_usage_inode()?)?;
    assert!(usage.contains("staged=1\n"));
    assert!(usage.contains("queued=1\n"));
    assert!(usage.contains("drained=1\n"));
    assert!(usage.contains("errors=0\n"));
    assert!(usage.contains("denied=0\n"));
    Ok(())
}

#[test]
fn control_drain_materializes_provider_model_errors() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::local_execution_provider_spec()?;
    set_default_provider(&fs, &provider)?;
    let unsupported_model = format!("not-{}", provider.default_model);
    fs.create_staged_request(
        "openai.chat",
        "bad-model.tmp",
        format!("{{\"model\":\"{unsupported_model}\",\"messages\":[]}}\n").as_str(),
    )?;
    fs.submit_request("openai.chat", "bad-model.tmp", "bad-model.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime
            .lookup_child(outbox, "bad-model.resp.json")
            .is_none(),
        "provider errors must not create success responses"
    );
    let error = runtime
        .lookup_child(outbox, "bad-model.error")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(error.contains("\"request_id\":\"bad-model\""));
    assert!(error.contains("unsupported provider model"));
    drop(runtime);
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("last_drained")?)?,
        "bad-model\n"
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"event\":\"error\""));
    assert!(audit.contains("\"fingerprint\":\"fnv1a64:"));
    assert!(audit.contains(&format!("\"provider\":\"{}\"", provider.id)));
    assert!(audit.contains(&format!("\"model\":\"{}\"", provider.default_model)));
    assert!(audit.contains("\"decision\":\"ready\""));
    Ok(())
}

#[test]
fn control_drain_materializes_provider_transport_errors() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::local_execution_provider_spec()?;
    set_default_provider(&fs, &provider)?;
    fs.create_staged_request("openai.chat", "transport-error.tmp", "{\"messages\":[]}\n")?;
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
    fs.submit_request(
        "openai.chat",
        "transport-error.tmp",
        "transport-error.req.json",
    )?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime
            .lookup_child(outbox, "transport-error.resp.json")
            .is_none(),
        "provider errors must not create success responses"
    );
    let error = runtime
        .lookup_child(outbox, "transport-error.error")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(error.contains("\"request_id\":\"transport-error\""));
    assert!(error.contains("unsupported provider format"));
    drop(runtime);
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("last_drained")?)?,
        "transport-error\n"
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"event\":\"error\""));
    assert!(audit.contains("\"name\":\"transport-error\""));
    assert!(audit.contains(&format!("\"provider\":\"{}\"", provider.id)));
    Ok(())
}

#[test]
fn control_drain_honors_request_model_field() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::local_execution_provider_spec()?;
    set_default_provider(&fs, &provider)?;
    fs.create_staged_request(
        "openai.chat",
        "with-model.tmp",
        format!(
            "{{\"model\":\"{}\",\"messages\":[]}}\n",
            provider.default_model
        )
        .as_str(),
    )?;
    fs.submit_request("openai.chat", "with-model.tmp", "with-model.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let outbox = fs
        .tree
        .path_inode(&["home", "1000", "api", "openai.chat", "outbox"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let response = runtime
        .lookup_child(outbox, "with-model.resp.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(response.contains(&format!("\"model\":\"{}\"", provider.default_model)));
    assert!(runtime.lookup_child(outbox, "with-model.error").is_none());
    drop(runtime);
    Ok(())
}
