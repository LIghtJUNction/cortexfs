use crate::CortexFs;

#[test]
fn simple_control_nodes_update_last_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for control_name in ["reload", "flush", "gc"] {
        let inode = fs.control_file_inode(control_name)?;
        {
            let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
            runtime.write(inode, 0, b"1\n")?;
        }
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("{control_name}\n")
        );
        assert!(
            fs.node_content(fs.audit_events_inode()?)?
                .contains(&format!("\"name\":\"{control_name}\""))
        );
    }
    let usage = fs.node_content(fs.audit_usage_inode()?)?;
    assert!(usage.contains("events=3\n"));
    assert!(usage.contains("staged=0\n"));
    assert!(usage.contains("queued=0\n"));
    assert!(usage.contains("drained=0\n"));
    assert!(usage.contains("errors=0\n"));
    assert!(usage.contains("denied=0\n"));
    Ok(())
}

#[test]
fn control_command_nodes_are_write_only() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let controls = [
        fs.control_file_inode("drain")?,
        fs.control_file_inode("reload")?,
        fs.user_control_file_inode("reload")?,
        fs.demo_thread_control_file_inode("pause")?,
        fs.demo_tool_loop_control_file_inode("pause")?,
        fs.agent_helper_control_file_inode("start")?,
        fs.cluster_control_file_inode("pause")?,
        fs.provider_models_file_inode(crate::default_provider_id(), "refresh")?,
        fs.provider_health_file_inode(crate::default_provider_id(), "check")?,
        fs.provider_secrets_file_inode(crate::default_provider_id(), "rotate")?,
    ];

    for inode in controls {
        assert_eq!(fs.node_attr(inode)?.perm, 0o222);
        assert_eq!(
            fs.node_content(inode),
            Err(fuse3::Errno::from(libc::EACCES))
        );
    }
    Ok(())
}

#[test]
fn simple_control_nodes_reject_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let reload = fs.control_file_inode("reload")?;

    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime.write(reload, 0, b"yes\n").is_err(),
        "control nodes accept only 1"
    );
    assert!(
        runtime.write(reload, 1, b"1\n").is_err(),
        "control nodes require offset zero"
    );
    drop(runtime);
    Ok(())
}

#[test]
fn user_space_control_nodes_update_last_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for control_name in ["reload", "gc"] {
        let inode = fs.user_control_file_inode(control_name)?;
        {
            let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
            runtime.write(inode, 0, b"1\n")?;
        }
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("spaces/users/1000/{control_name}\n")
        );
        let audit = fs.node_content(fs.audit_events_inode()?)?;
        assert!(audit.contains("\"format\":\"space.users/1000.control\""));
        assert!(audit.contains(&format!("\"name\":\"{control_name}\"")));
    }
    Ok(())
}

#[test]
fn user_space_control_nodes_reject_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let reload = fs.user_control_file_inode("reload")?;

    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime.write(reload, 0, b"yes\n"),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    assert_eq!(
        runtime.write(reload, 1, b"1\n"),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    drop(runtime);
    Ok(())
}
