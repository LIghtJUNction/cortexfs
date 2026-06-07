use crate::CortexFs;

fn assert_database_status_runtime_file(
    fs: &CortexFs,
    database: &'static str,
    expected: &'static str,
) -> fuse3::Result<fuse3::Inode> {
    assert!(
        fs.tree.path_inode(&["db", database, "status"]).is_none(),
        "{database} status must be runtime-owned, not a static placeholder"
    );
    let database_dir = fs.path_inode(["db", database])?;
    let entries = fs.children(database_dir);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.name.to_str() == Some("status"))
            .count(),
        1,
        "{database} directory must expose one status entry"
    );
    let status = fs.resolve_path_inode(["db", database, "status"])?;
    assert_eq!(fs.node_content(status)?, expected);
    Ok(status)
}

#[test]
fn postgres_dsn_current_updates_effective_without_exposing_password() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    assert_database_status_runtime_file(&fs, "sqlite", "disabled\n")?;
    let status = assert_database_status_runtime_file(&fs, "postgres", "disabled\n")?;
    let dsn = fs
        .tree
        .path_inode(crate::POSTGRES_DSN_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let current = runtime
        .lookup_child(dsn, "current")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    runtime.write(
        current,
        0,
        b"postgres://cortex:secret-password@localhost:5432/cortex\n",
    )?;
    assert_eq!(
        runtime
            .lookup_child(dsn, "source")
            .and_then(crate::Node::content),
        Some("current\n")
    );
    let effective = runtime
        .lookup_child(dsn, "effective")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(effective.contains("postgres://cortex:***@localhost:5432/cortex"));
    assert!(!effective.contains("secret-password"));
    drop(runtime);
    assert_eq!(fs.node_content(status)?, "configured\n");
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"database.postgres.dsn\""));
    assert!(!audit.contains("secret-password"));
    Ok(())
}
