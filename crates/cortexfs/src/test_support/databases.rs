use crate::CortexFs;

#[test]
fn postgres_dsn_current_updates_effective_without_exposing_password() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let status = fs.path_inode(["databases", "postgres", "status"])?;
    let dsn = fs
        .tree
        .path_inode(crate::POSTGRES_DSN_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(fs.node_content(status)?, "disabled\n");
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
