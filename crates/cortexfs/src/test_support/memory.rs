use crate::CortexFs;

#[test]
fn projection_exposes_memory_vector_and_database_shape() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_memory_projection(&fs)?;
    assert_vector_projection(&fs)?;
    assert_database_projection(&fs)?;
    Ok(())
}

fn assert_memory_projection(fs: &CortexFs) -> fuse3::Result<()> {
    assert_eq!(
        fs.lookup_path(["memory", "context"])
            .and_then(crate::Node::content),
        Some("local:memory_r:memory_t:s0\n")
    );
    assert_eq!(
        fs.lookup_path(["memory", "layers"])
            .and_then(crate::Node::content),
        Some("working\nepisodic\nsemantic\nprocedural\nprofile\n")
    );
    for directory in [
        "working",
        "episodic",
        "semantic",
        "procedural",
        "profile",
        "indexes",
    ] {
        assert!(
            fs.lookup_path(["memory", directory]).is_some(),
            "global memory layer must exist"
        );
    }
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "threads", "demo", "memory_scope"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_USER_MEMORY_SCOPE_TEXT)
    );
    assert!(
        fs.lookup_path(["spaces", "users", "1000", "memory", "search", "query"])
            .is_none(),
        "memory search query is runtime-backed, not static"
    );
    let search = fs
        .tree
        .path_inode(crate::MEMORY_SEARCH_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        fs.lookup_path(["spaces", "users", "1000", "memory", "semantic", "inbox"])
            .is_some(),
        "semantic memory inbox must accept file submissions"
    );
    let semantic = fs
        .tree
        .path_inode(crate::MEMORY_SEMANTIC_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime.lookup_child(search, "query").is_some(),
        "space memory search query node must exist"
    );
    assert!(
        runtime.lookup_child(search, "results.jsonl").is_some(),
        "space memory search results node must exist"
    );
    assert!(
        runtime.lookup_child(semantic, "items.jsonl").is_some(),
        "semantic memory items file must be runtime-backed"
    );
    for layer in ["working", "episodic", "procedural", "profile"] {
        let parent = fs.path_inode(["spaces", "users", "1000", "memory", layer])?;
        assert!(
            runtime.lookup_child(parent, "items.jsonl").is_some(),
            "{layer} memory items file must be runtime-backed"
        );
    }
    drop(runtime);
    Ok(())
}

fn assert_vector_projection(fs: &CortexFs) -> fuse3::Result<()> {
    assert_eq!(
        fs.lookup_path(["vector", "stores", "pgvector", "distance"])
            .and_then(crate::Node::content),
        Some("cosine\n")
    );
    assert_eq!(
        fs.lookup_path(["vector", "stores", "count"])
            .and_then(crate::Node::content),
        Some("4\n")
    );
    assert_eq!(
        fs.lookup_path(["vector", "stores", "list"])
            .and_then(crate::Node::content),
        Some("local\npgvector\nqdrant\nmilvus\n")
    );
    assert_eq!(
        fs.lookup_path(["vector", "context"])
            .and_then(crate::Node::content),
        Some("local:vector_r:vector_index_t:s0\n")
    );
    assert!(
        fs.lookup_path(["vector", "stores", "pgvector", "refresh"])
            .and_then(crate::Node::content)
            .is_some(),
        "pgvector refresh keeps a static ABI path"
    );
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime
            .node(fs.path_inode(["vector", "stores", "pgvector", "enabled"])?)
            .and_then(crate::Node::content),
        Some("0\n"),
        "pgvector enabled is runtime-backed"
    );
    assert_eq!(
        runtime
            .node(fs.path_inode(["vector", "stores", "pgvector", "status"])?)
            .and_then(crate::Node::content),
        Some("disabled\n"),
        "pgvector status is runtime-backed"
    );
    assert_eq!(
        runtime
            .node(fs.path_inode(["vector", "stores", "pgvector", "collections"])?)
            .and_then(crate::Node::content),
        Some("\n"),
        "pgvector collections is runtime-backed"
    );
    drop(runtime);
    Ok(())
}

fn assert_database_projection(fs: &CortexFs) -> fuse3::Result<()> {
    let dsn = fs
        .tree
        .path_inode(crate::POSTGRES_DSN_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime.lookup_child(dsn, "current").is_some(),
        "postgres dsn current must be runtime-backed"
    );
    assert!(
        runtime.lookup_child(dsn, "effective").is_some(),
        "postgres dsn effective must be runtime-backed"
    );
    assert_eq!(
        runtime
            .lookup_child(dsn, "source")
            .and_then(crate::Node::content),
        Some("unset\n")
    );
    drop(runtime);
    assert_eq!(
        fs.lookup_path(["databases", "context"])
            .and_then(crate::Node::content),
        Some("local:database_r:database_t:s0\n")
    );
    assert_eq!(
        fs.lookup_path(["databases", "count"])
            .and_then(crate::Node::content),
        Some("2\n")
    );
    assert_eq!(
        fs.lookup_path(["databases", "list"])
            .and_then(crate::Node::content),
        Some("sqlite\npostgres\n")
    );
    Ok(())
}

#[test]
fn pgvector_store_enabled_and_refresh_update_runtime_view() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let enabled = fs.path_inode(["vector", "stores", "pgvector", "enabled"])?;
    let status = fs.path_inode(["vector", "stores", "pgvector", "status"])?;
    let collections = fs.path_inode(["vector", "stores", "pgvector", "collections"])?;
    let refresh = fs.path_inode(["vector", "stores", "pgvector", "refresh"])?;

    assert_eq!(fs.node_attr(enabled)?.perm, 0o644);
    assert_eq!(fs.node_attr(refresh)?.perm, 0o222);
    assert!(fs.node_content(refresh).is_err());

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(runtime.write(enabled, 0, b"1\n")?, 2);
        assert_eq!(runtime.write(refresh, 0, b"1\n")?, 2);
        drop(runtime);
    }

    assert_eq!(fs.node_content(enabled)?, "1\n");
    assert_eq!(fs.node_content(status)?, "ready\n");
    assert_eq!(fs.node_content(collections)?, "memory_semantic\n");
    assert_eq!(
        fs.node_content(fs.control_file_inode("last_control")?)?,
        "vector/stores/pgvector/refresh\n"
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"vector.store.pgvector\""));
    assert!(audit.contains("\"name\":\"enabled\""));
    assert!(audit.contains("\"name\":\"refresh\""));
    assert!(audit.contains("\"event\":\"refreshed\""));
    Ok(())
}

#[test]
fn pgvector_store_control_nodes_reject_invalid_writes() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let enabled = fs.path_inode(["vector", "stores", "pgvector", "enabled"])?;
    let refresh = fs.path_inode(["vector", "stores", "pgvector", "refresh"])?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    assert!(runtime.write(enabled, 0, b"true\n").is_err());
    assert!(runtime.write(enabled, 1, b"1\n").is_err());
    assert!(runtime.write(refresh, 0, b"yes\n").is_err());
    assert!(runtime.write(refresh, 1, b"1\n").is_err());
    drop(runtime);
    Ok(())
}

#[test]
fn memory_search_query_derives_results_from_thread_messages() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_thread_request(
        "memory-001.tmp",
        "{\"messages\":[{\"role\":\"user\",\"content\":\"remember cortexfs\"}]}\n",
    )?;
    fs.submit_thread_request("memory-001.tmp", "memory-001.req.json")?;
    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let search = fs
        .tree
        .path_inode(crate::MEMORY_SEARCH_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let query = runtime
        .lookup_child(search, "query")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    runtime.write(query, 0, b"remember")?;
    let results = runtime
        .lookup_child(search, "results.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(results.contains("\"source\":\"threads/demo/messages.jsonl\""));
    assert!(results.contains("\"line\":1"));
    assert!(results.contains("remember cortexfs"));
    drop(runtime);
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"memory.search\"")
    );
    Ok(())
}

#[test]
fn memory_search_indexes_runtime_backed_non_semantic_layers() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let search = fs
        .tree
        .path_inode(crate::MEMORY_SEARCH_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    runtime.append_memory_layer_item(
        "working",
        "working-001",
        "working cortexfs scratchpad",
        "fnv1a64:working",
    );
    runtime.append_memory_layer_item(
        "profile",
        "profile-001",
        "profile cortexfs preference",
        "fnv1a64:profile",
    );
    let query = runtime
        .lookup_child(search, "query")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    runtime.write(query, 0, b"cortexfs")?;
    let results = runtime
        .lookup_child(search, "results.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(results.contains("\"source\":\"memory/working/items.jsonl\""));
    assert!(results.contains("working cortexfs scratchpad"));
    assert!(results.contains("\"source\":\"memory/profile/items.jsonl\""));
    assert!(results.contains("profile cortexfs preference"));
    drop(runtime);

    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"memory.search\"")
    );
    Ok(())
}

#[test]
fn semantic_memory_item_submit_drains_into_items_and_search() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_memory_item(
        "semantic-001.tmp",
        "{\"text\":\"semantic cortexfs memory\",\"tags\":[\"design\"]}\n",
    )?;
    fs.submit_memory_item("semantic-001.tmp", "semantic-001.req.json")?;
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );

    let semantic = fs
        .tree
        .path_inode(crate::MEMORY_SEMANTIC_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let items = runtime
            .lookup_child(semantic, "items.jsonl")
            .and_then(crate::Node::content)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        assert!(
            items.is_empty(),
            "semantic memory submit queues work until control/drain"
        );
        drop(runtime);
    }

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let search = fs
        .tree
        .path_inode(crate::MEMORY_SEARCH_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let items = runtime
        .lookup_child(semantic, "items.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(items.contains("\"layer\":\"semantic\""));
    assert!(items.contains("semantic cortexfs memory"));
    let query = runtime
        .lookup_child(search, "query")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    runtime.write(query, 0, b"semantic cortexfs")?;
    let results = runtime
        .lookup_child(search, "results.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(results.contains("\"source\":\"memory/semantic/items.jsonl\""));
    assert!(results.contains("semantic cortexfs memory"));
    drop(runtime);

    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"memory.semantic\"")
    );
    Ok(())
}
