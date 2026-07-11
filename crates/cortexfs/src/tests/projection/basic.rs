#[test]
fn fuse_v1_projection_root_is_traversable_when_backing_root_is_private() {
    let root = reference_tree("fuse-v1-private-backing-root");
    assert!(fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    let attr = projection.getattr("");
    assert!(matches!(
        attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Directory
                && attr.mode() & 0o777 == 0o755
    ));

    let status_attr = projection.getattr("status");
    assert!(matches!(status_attr, Ok(ref attr) if attr.mode() & 0o777 == 0o644));
}

#[test]
fn fuse_v1_projection_exposes_sticky_agent_creation_directory() {
    let root = reference_tree("fuse-v1-agent-sticky-directory");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert!(matches!(
        projection.getattr("agent"),
        Ok(ref attr) if attr.mode() & 0o7777 == 0o1777
    ));
    assert!(matches!(
        fs::metadata(root.join("agent"))
            .map(|metadata| metadata.permissions().mode() & 0o7777),
        Ok(mode) if mode != 0o1777
    ));
}

#[test]
fn fuse_v1_projection_allows_durable_session_record_writes() {
    let root = reference_tree("fuse-v1-session-record-writes");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let session = "home/1234/agent/coder/session/fuse";
    let index = "home/1234/agent/coder/session/index";
    let session_dir = root.join(session);
    let index_dir = root.join(index);
    assert!(fs::create_dir_all(&session_dir).is_ok());
    assert!(fs::create_dir_all(index_dir.join("by-cwd")).is_ok());
    assert!(fs::write(session_dir.join("messages.jsonl"), "").is_ok());
    assert!(fs::write(session_dir.join("events.jsonl"), "").is_ok());
    assert!(fs::write(session_dir.join("state"), "idle\n").is_ok());
    assert!(fs::write(index_dir.join("list"), "").is_ok());
    assert!(fs::write(index_dir.join("current"), "old\n").is_ok());

    assert!(
        projection
            .write_control_file_at(
                &format!("{session}/messages.jsonl"),
                0,
                b"{\"role\":\"user\"}\n",
            )
            .is_ok()
    );
    let message_len =
        fs::metadata(session_dir.join("messages.jsonl")).map_or(0, |metadata| metadata.len());
    assert!(
        projection
            .write_control_file_at(
                &format!("{session}/messages.jsonl"),
                message_len,
                b"{\"role\":\"assistant\"}\n",
            )
            .is_ok()
    );
    assert!(
        projection
            .write_control_file_at(&format!("{session}/state"), 0, b"active\n")
            .is_ok()
    );
    assert!(
        projection
            .write_control_file_at(&format!("{index}/current"), 0, b"fuse\n")
            .is_ok()
    );
    assert!(
        projection
            .write_control_file_at(&format!("{index}/by-cwd/cwd-test"), 0, b"fuse\n")
            .is_ok()
    );

    assert_eq!(
        projection.write_control_file_at(&format!("{session}/state"), 1, b"done\n"),
        Err(FuseV1Error::InvalidOffset)
    );
    assert_eq!(
        projection.write_control_file_at(&format!("{session}/messages.jsonl"), 1, b"bad\n"),
        Err(FuseV1Error::InvalidOffset)
    );
    assert_eq!(
        projection.write_control_file_at(
            "home/1234/agent/coder/session/fuse/not-a-session-control",
            0,
            b"bad\n",
        ),
        Err(FuseV1Error::NotControlFile)
    );

    assert_eq!(
        fs::read_to_string(session_dir.join("messages.jsonl")).unwrap_or_default(),
        "{\"role\":\"user\"}\n{\"role\":\"assistant\"}\n"
    );
    assert_eq!(
        fs::read_to_string(session_dir.join("state")).unwrap_or_default(),
        "active\n"
    );
    assert_eq!(
        fs::read_to_string(index_dir.join("current")).unwrap_or_default(),
        "fuse\n"
    );
}

#[test]
fn fuse_v1_projection_allows_durable_session_layout_creation() {
    let root = reference_tree("fuse-v1-session-layout-create");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let session = "home/1000/agent/coder/session/fuse";
    assert!(fs::remove_dir_all(root.join("home/1000/agent/coder/session/index")).is_ok());

    for dir in [
        session,
        &format!("{session}/context"),
        &format!("{session}/context/pinned"),
        &format!("{session}/context/swap"),
        &format!("{session}/context/swap/chunk"),
        &format!("{session}/context/dedup"),
        &format!("{session}/context/dedup/blob"),
        &format!("{session}/context/child"),
        "home/1000/agent/coder/session/index",
        "home/1000/agent/coder/session/index/by-cwd",
        "home/1000/agent/coder/session/index/by-hash",
        "home/1000/agent/coder/session/index/by-uuid",
    ] {
        assert_eq!(projection.create_layout_dir(dir, 1000, 1000, 0o700), Ok(()));
    }

    for file in [
        &format!("{session}/messages.jsonl"),
        &format!("{session}/events.jsonl"),
        &format!("{session}/latest.md"),
        &format!("{session}/state"),
        &format!("{session}/cwd"),
        &format!("{session}/workspace"),
        &format!("{session}/created_at"),
        &format!("{session}/updated_at"),
        &format!("{session}/meta.json"),
        &format!("{session}/context/budget"),
        &format!("{session}/context/pack.json"),
        &format!("{session}/context/pack.md"),
        &format!("{session}/context/summary.md"),
        &format!("{session}/context/facts.jsonl"),
        &format!("{session}/context/decisions.jsonl"),
        &format!("{session}/context/todo.md"),
        &format!("{session}/context/refs.jsonl"),
        &format!("{session}/context/swap/index.jsonl"),
        &format!("{session}/context/dedup/index.jsonl"),
        "home/1000/agent/coder/session/index/list",
        "home/1000/agent/coder/session/index/current",
        "home/1000/agent/coder/session/index/by-cwd/workspace",
    ] {
        assert_eq!(
            projection.create_layout_file(file, 1000, 1000, 0o600),
            Ok(())
        );
    }

    assert!(root.join(session).join("messages.jsonl").is_file());
    assert!(root.join(session).join("context/swap/chunk").is_dir());
    assert_eq!(projection.set_layout_mode(session, 0o700, 1000), Ok(()));
    assert_eq!(
        projection.set_layout_mode(&format!("{session}/messages.jsonl"), 0o600, 1000),
        Ok(())
    );
    assert!(matches!(
        fs::metadata(root.join(session)).map(|metadata| metadata.permissions().mode() & 0o777),
        Ok(0o700)
    ));
    assert!(matches!(
        fs::metadata(root.join(session).join("messages.jsonl"))
            .map(|metadata| metadata.permissions().mode() & 0o777),
        Ok(0o600)
    ));
    assert_eq!(
        projection.create_layout_dir("home/1000/tool/not-session", 1000, 1000, 0o700),
        Err(FuseV1Error::NotControlFile)
    );
    assert_eq!(
        projection.create_layout_file("home/1000/agent/coder/session/fuse/bad", 1000, 1000, 0o600,),
        Err(FuseV1Error::NotControlFile)
    );
}

#[test]
fn fuse_v1_projection_creates_owned_agent_lifecycle_paths() {
    let root = reference_tree("fuse-v1-agent-lifecycle-create");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();
    let control = "agent/scratch.d";
    let owner_temp = "agent/scratch.d/.owner.tmp-1-1-0";
    let wrapper_temp = "agent/.scratch.tmp-1-1-0";

    assert_eq!(
        projection.create_layout_dir(control, uid, gid, 0o755),
        Ok(())
    );
    assert_eq!(
        projection.create_layout_file(owner_temp, uid, gid, 0o644),
        Ok(())
    );
    assert_eq!(
        projection.write_fuse_file_at_for_owner(
            owner_temp,
            0,
            uid.to_string().as_bytes(),
            uid,
            gid
        ),
        Ok(())
    );
    assert_eq!(
        projection.rename_atomic_temp(owner_temp, "agent/scratch.d/owner", uid),
        Ok(())
    );
    assert_eq!(
        projection.create_layout_file(wrapper_temp, uid, gid, 0o600),
        Ok(())
    );
    assert!(
        !projection
            .readdir("agent")
            .unwrap_or_default()
            .iter()
            .any(|entry| entry.name() == ".scratch.tmp-1-1-0")
    );
    assert_eq!(
        projection.write_fuse_file_at_for_owner(wrapper_temp, 0, b"#!/bin/sh\n", uid, gid),
        Ok(())
    );
    assert_eq!(
        projection.rename_atomic_temp(wrapper_temp, "agent/scratch", uid),
        Ok(())
    );
    assert_eq!(
        projection.set_layout_mode("agent/scratch", 0o755, uid),
        Ok(())
    );
    assert!(matches!(
        projection.getattr("agent/scratch"),
        Ok(ref attr) if attr.uid() == uid && attr.gid() == gid && attr.mode() == 0o555
    ));
    for directory in [
        format!("home/{uid}/agent/scratch"),
        format!("home/{uid}/agent/scratch/root"),
        format!("home/{uid}/agent/scratch/session"),
        format!("home/{uid}/agent/scratch/session/index"),
        format!("home/{uid}/agent/scratch/session/index/by-cwd"),
        format!("home/{uid}/agent/scratch/data"),
        format!("home/{uid}/agent/scratch/cache"),
        format!("home/{uid}/agent/scratch/log"),
    ] {
        assert_eq!(
            projection.create_layout_dir(&directory, uid, gid, 0o755),
            Ok(())
        );
    }

    assert!(root.join("agent/scratch").is_file());
    assert_eq!(
        fs::read_to_string(root.join("agent/scratch.d/owner")).unwrap_or_default(),
        uid.to_string()
    );
    assert!(
        root.join(format!("home/{uid}/agent/scratch/session/index/by-cwd"))
            .is_dir()
    );
}

#[test]
fn fuse_v1_projection_directory_creation_is_exclusive() {
    let root = reference_tree("fuse-v1-agent-lifecycle-exclusive-dir");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();
    let path = root.join("agent/scratch.d");
    assert!(fs::create_dir_all(&path).is_ok());
    assert!(fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).is_ok());
    let before = fs::symlink_metadata(&path).ok().map(|metadata| {
        (
            metadata.uid(),
            metadata.gid(),
            metadata.permissions().mode() & 0o7777,
        )
    });

    assert_eq!(
        projection.create_layout_dir("agent/scratch.d", uid, gid, 0o755),
        Err(FuseV1Error::AlreadyExists)
    );
    assert_eq!(
        projection.create_layout_dir(
            "agent/scratch.d",
            uid.saturating_add(1),
            gid.saturating_add(1),
            0o777,
        ),
        Err(FuseV1Error::PermissionDenied)
    );
    assert_eq!(
        fs::symlink_metadata(path).ok().map(|metadata| (
            metadata.uid(),
            metadata.gid(),
            metadata.permissions().mode() & 0o7777,
        )),
        before
    );
}

#[test]
fn fuse_v1_projection_rejects_agent_control_owner_reassignment() {
    let root = reference_tree("fuse-v1-agent-owner-reassignment");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();
    let first = "agent/scratch.d/.owner.tmp-1-1-0";
    let second = "agent/scratch.d/.owner.tmp-2-2-0";

    assert_eq!(
        projection.create_layout_dir("agent/scratch.d", uid, gid, 0o755),
        Ok(())
    );
    assert_eq!(
        projection.create_layout_file(first, uid, gid, 0o644),
        Ok(())
    );
    assert_eq!(
        projection.write_fuse_file_at_for_owner(first, 0, uid.to_string().as_bytes(), uid, gid),
        Ok(())
    );
    assert_eq!(
        projection.rename_atomic_temp(first, "agent/scratch.d/owner", uid),
        Ok(())
    );
    assert_eq!(
        projection.create_layout_file(second, uid, gid, 0o644),
        Ok(())
    );

    assert_eq!(
        projection.write_fuse_file_at_for_owner(
            second,
            0,
            uid.saturating_add(1).to_string().as_bytes(),
            uid,
            gid,
        ),
        Err(FuseV1Error::PermissionDenied)
    );
}

#[test]
fn fuse_v1_projection_persists_owned_agent_and_terminal_socket_aliases() {
    let root = reference_tree("fuse-v1-agent-socket-aliases");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();
    assert!(fs::write(root.join("agent/coder.d/owner"), format!("{uid}\n")).is_ok());
    let agent_target = PathBuf::from(format!(
        "/run/user/{uid}/cortexfs/agent/root-hash/coder.sock"
    ));
    let terminal = format!("home/{uid}/agent/coder/session/test/terminal");
    assert!(fs::create_dir_all(root.join(format!("home/{uid}/agent/coder/session/test"))).is_ok());
    assert_eq!(
        projection.create_layout_dir(&terminal, uid, gid, 0o755),
        Ok(())
    );
    let terminal_target = PathBuf::from(format!(
        "/run/user/{uid}/cortexfs/terminal/coder/test/main.sock"
    ));

    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", uid),
        Ok(())
    );
    assert!(
        projection
            .set_socket_alias("agent/coder.sock", &agent_target, uid, gid)
            .is_ok()
    );
    assert!(
        projection
            .set_socket_alias(&format!("{terminal}/main.sock"), &terminal_target, uid, gid,)
            .is_ok()
    );

    assert_eq!(projection.readlink("agent/coder.sock"), Ok(agent_target));
    assert_eq!(
        projection.readlink(&format!("{terminal}/main.sock")),
        Ok(terminal_target)
    );
}

#[test]
fn fuse_v1_projection_renames_owner_socket_alias_to_generated_claim_and_back() {
    let root = reference_tree("fuse-v1-socket-alias-claim");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    assert!(fs::write(root.join("agent/coder.d/owner"), format!("{uid}\n")).is_ok());
    let target = PathBuf::from(format!(
        "/run/user/{uid}/cortexfs/agent/root-hash/coder.sock"
    ));
    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", uid),
        Ok(())
    );
    assert!(symlink(&target, root.join("agent/coder.sock")).is_ok());
    let claim = "agent/.coder.sock.claim-1-1-0";

    assert_eq!(
        projection.rename_socket_alias_claim("agent/coder.sock", claim, uid),
        Ok(())
    );
    assert!(fs::symlink_metadata(root.join("agent/coder.sock")).is_err());
    assert!(matches!(fs::read_link(root.join(claim)), Ok(ref value) if value == &target));
    assert_eq!(
        projection.rename_socket_alias_claim(claim, "agent/coder.sock", uid),
        Ok(())
    );
    assert!(matches!(
        fs::read_link(root.join("agent/coder.sock")),
        Ok(ref value) if value == &target
    ));
}

#[test]
fn fuse_v1_projection_rejects_foreign_socket_claim_without_moving_alias() {
    let root = reference_tree("fuse-v1-socket-alias-claim-foreign");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    assert!(fs::write(root.join("agent/coder.d/owner"), format!("{uid}\n")).is_ok());
    let target = PathBuf::from(format!(
        "/run/user/{uid}/cortexfs/agent/root-hash/coder.sock"
    ));
    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", uid),
        Ok(())
    );
    assert!(symlink(&target, root.join("agent/coder.sock")).is_ok());

    assert_eq!(
        projection.rename_socket_alias_claim(
            "agent/coder.sock",
            "agent/.coder.sock.claim-1-1-0",
            uid.saturating_add(1),
        ),
        Err(FuseV1Error::PermissionDenied)
    );
    assert_eq!(
        projection.rename_socket_alias_claim(
            "agent/coder.sock",
            "agent/.reviewer.sock.claim-1-1-0",
            uid,
        ),
        Err(FuseV1Error::NotControlFile)
    );
    assert!(matches!(
        fs::read_link(root.join("agent/coder.sock")),
        Ok(ref value) if value == &target
    ));
}

#[test]
fn fuse_v1_projection_hides_but_resolves_and_removes_generated_socket_claim() {
    let root = reference_tree("fuse-v1-socket-alias-claim-remove");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    assert!(fs::write(root.join("agent/coder.d/owner"), format!("{uid}\n")).is_ok());
    let target = PathBuf::from(format!(
        "/run/user/{uid}/cortexfs/agent/root-hash/coder.sock"
    ));
    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", uid),
        Ok(())
    );
    assert!(symlink(&target, root.join("agent/coder.sock")).is_ok());
    let claim = "agent/.coder.sock.claim-1-1-0";
    assert_eq!(
        projection.rename_socket_alias_claim("agent/coder.sock", claim, uid),
        Ok(())
    );

    assert!(projection.node_for_path(claim).is_ok());
    assert!(projection.readdir("agent").is_ok_and(|entries| {
        entries
            .iter()
            .all(|entry| entry.name() != ".coder.sock.claim-1-1-0")
    }));
    assert_eq!(projection.remove_socket_alias_claim(claim, uid), Ok(()));
    assert_eq!(projection.node_for_path(claim), Err(FuseV1Error::NotFound));
}

#[test]
fn fuse_v1_projection_rejects_invalid_foreign_and_regular_socket_claims() {
    let root = reference_tree("fuse-v1-socket-alias-claim-invalid");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    assert!(fs::write(root.join("agent/coder.d/owner"), format!("{uid}\n")).is_ok());
    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", uid),
        Ok(())
    );
    assert!(fs::write(root.join("agent/coder.sock"), "not a socket").is_ok());
    let claim = "agent/.coder.sock.claim-1-1-0";

    assert_eq!(
        projection.rename_socket_alias_claim("agent/coder.sock", claim, uid),
        Err(FuseV1Error::InvalidPath)
    );
    assert!(root.join("agent/coder.sock").is_file());
    assert!(
        fs::rename(
            root.join("agent/coder.sock"),
            root.join("agent/.coder.sock.claim-1-1-0")
        )
        .is_ok()
    );
    assert_eq!(
        projection.remove_socket_alias_claim(claim, uid.saturating_add(1)),
        Err(FuseV1Error::PermissionDenied)
    );
    assert_eq!(
        projection.remove_socket_alias_claim(claim, uid),
        Err(FuseV1Error::InvalidPath)
    );
    assert!(root.join(claim).is_file());
    assert!(!FuseV1Projection::is_socket_alias_claim_path(
        "agent/.coder.sock.claim-invalid"
    ));
}

#[test]
fn fuse_v1_projection_session_noreplace_create_preserves_existing_target() {
    let root = reference_tree("fuse-v1-session-noreplace");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();
    assert!(fs::write(root.join("agent/coder.d/owner"), format!("{uid}\n")).is_ok());
    let session = format!("home/{uid}/agent/coder/session/fuse");
    assert!(fs::create_dir_all(root.join(&session)).is_ok());
    let first = format!("{session}/.state.tmp-1-1-0");
    let second = format!("{session}/.state.tmp-2-2-0");
    let target = format!("{session}/state");

    assert_eq!(
        projection.create_layout_file(&first, uid, gid, 0o600),
        Ok(())
    );
    assert_eq!(
        projection.write_fuse_file_at_for_owner(&first, 0, b"first\n", uid, gid),
        Ok(())
    );
    assert_eq!(
        projection.rename_atomic_temp_noreplace(&first, &target, uid),
        Ok(())
    );
    assert_eq!(
        projection.create_layout_file(&second, uid, gid, 0o600),
        Ok(())
    );
    assert_eq!(
        projection.write_fuse_file_at_for_owner(&second, 0, b"second\n", uid, gid),
        Ok(())
    );
    assert_eq!(
        projection.rename_atomic_temp_noreplace(&second, &target, uid),
        Err(FuseV1Error::AlreadyExists)
    );
    assert_eq!(
        fs::read_to_string(root.join(&target)).unwrap_or_default(),
        "first\n"
    );
    assert_eq!(
        fs::read_to_string(root.join(&second)).unwrap_or_default(),
        "second\n"
    );
}

#[test]
fn fuse_v1_projection_persists_owned_agent_socket_placeholder() {
    let root = reference_tree("fuse-v1-agent-socket-placeholder");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();
    assert!(fs::write(root.join("agent/coder.d/owner"), format!("{uid}\n")).is_ok());
    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", uid),
        Ok(())
    );

    assert!(
        projection
            .create_socket_placeholder("agent/coder.sock", uid, gid, 0o755)
            .is_ok()
    );

    assert!(matches!(
        fs::symlink_metadata(root.join("agent/coder.sock")),
        Ok(metadata)
            if metadata.file_type().is_socket()
                && metadata.uid() == uid
                && metadata.permissions().mode() & 0o7777 == 0o755
    ));
    assert_eq!(
        projection.set_socket_placeholder_mode("agent/coder.sock", uid, 0o777),
        Ok(())
    );
    assert!(matches!(
        fs::symlink_metadata(root.join("agent/coder.sock")),
        Ok(metadata) if metadata.permissions().mode() & 0o7777 == 0o777
    ));
    assert_eq!(
        projection.set_socket_placeholder_mode("agent/coder.sock", uid.saturating_add(1), 0o700),
        Err(FuseV1Error::PermissionDenied)
    );
    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", uid.saturating_add(1)),
        Err(FuseV1Error::PermissionDenied)
    );
    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", uid),
        Ok(())
    );
    let target = PathBuf::from(format!(
        "/run/user/{uid}/cortexfs/agent/root-hash/coder.sock"
    ));
    assert!(
        projection
            .set_socket_alias("agent/coder.sock", &target, uid, gid)
            .is_ok()
    );
    assert_eq!(
        projection.set_socket_placeholder_mode("agent/coder.sock", uid, 0o777),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", uid),
        Ok(())
    );
    assert!(fs::write(root.join("agent/coder.sock"), "not a socket\n").is_ok());
    assert_eq!(
        projection.set_socket_placeholder_mode("agent/coder.sock", uid, 0o777),
        Err(FuseV1Error::InvalidPath)
    );
}

#[test]
fn fuse_v1_projection_rejects_foreign_or_escaped_socket_aliases() {
    let root = reference_tree("fuse-v1-agent-socket-alias-security");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();
    assert!(fs::write(root.join("agent/coder.d/owner"), format!("{uid}\n")).is_ok());

    assert_eq!(
        projection.set_socket_alias(
            "agent/coder.sock",
            Path::new("/tmp/cortexfs/agent/coder.sock"),
            uid,
            gid,
        ),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        projection.set_socket_alias(
            &format!(
                "home/{}/agent/coder/session/test/terminal/main.sock",
                uid.saturating_add(1)
            ),
            &PathBuf::from(format!(
                "/run/user/{uid}/cortexfs/terminal/coder/test/main.sock"
            )),
            uid,
            gid,
        ),
        Err(FuseV1Error::PermissionDenied)
    );

    let outside = clean_test_dir("fuse-v1-agent-socket-alias-security-outside");
    assert!(fs::remove_dir_all(root.join("agent/coder.d")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(outside.join("owner"), format!("{uid}\n")).is_ok());
    assert!(symlink(&outside, root.join("agent/coder.d")).is_ok());
    assert_eq!(
        projection.set_socket_alias(
            "agent/coder.sock",
            &PathBuf::from(format!(
                "/run/user/{uid}/cortexfs/agent/root-hash/coder.sock"
            )),
            uid,
            gid,
        ),
        Err(FuseV1Error::InvalidPath)
    );
}

#[test]
fn fuse_v1_projection_cleans_only_new_socket_entries_after_chown_failure() {
    let uid = nix::unistd::Uid::current().as_raw();
    if uid == 0 {
        return;
    }
    let root = reference_tree("fuse-v1-agent-socket-chown-cleanup");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let foreign_uid = uid.saturating_add(1);
    let gid = nix::unistd::Gid::current().as_raw();
    assert!(fs::write(root.join("agent/coder.d/owner"), format!("{foreign_uid}\n"),).is_ok());
    assert_eq!(
        projection.remove_socket_alias("agent/coder.sock", foreign_uid),
        Ok(())
    );

    assert_eq!(
        projection.create_socket_placeholder("agent/coder.sock", foreign_uid, gid, 0o777),
        Err(FuseV1Error::Io)
    );
    assert!(fs::symlink_metadata(root.join("agent/coder.sock")).is_err());

    let existing_socket = std::os::unix::net::UnixListener::bind(root.join("agent/coder.sock"));
    assert!(existing_socket.is_ok());
    assert_eq!(
        projection.create_socket_placeholder("agent/coder.sock", foreign_uid, gid, 0o777),
        Err(FuseV1Error::Io)
    );
    assert!(matches!(
        fs::symlink_metadata(root.join("agent/coder.sock")),
        Ok(metadata) if metadata.file_type().is_socket()
    ));
    drop(existing_socket);
    assert!(fs::remove_file(root.join("agent/coder.sock")).is_ok());

    let target = PathBuf::from(format!(
        "/run/user/{foreign_uid}/cortexfs/agent/root-hash/coder.sock"
    ));
    assert_eq!(
        projection.set_socket_alias("agent/coder.sock", &target, foreign_uid, gid),
        Err(FuseV1Error::Io)
    );
    assert!(fs::symlink_metadata(root.join("agent/coder.sock")).is_err());

    assert!(symlink(&target, root.join("agent/coder.sock")).is_ok());
    assert_eq!(
        projection.set_socket_alias("agent/coder.sock", &target, foreign_uid, gid),
        Err(FuseV1Error::Io)
    );
    assert!(matches!(
        fs::read_link(root.join("agent/coder.sock")),
        Ok(existing) if existing == target
    ));
}

#[test]
fn fuse_v1_paths_reject_control_characters() {
    let root = Path::new("/ctx");

    assert_eq!(
        resolve_fuse_abi_path(root, "agent/coder\u{1b}.d/status"),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        resolve_fuse_abi_path(root, "agent/coder\r.d/status"),
        Err(FuseV1Error::InvalidPath)
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "single projection smoke test keeps related FUSE ABI assertions together"
)]
fn fuse_v1_projection_exposes_reference_tree_ops() {
    let root = reference_tree("fuse-v1-projection");
    write_fixture_file(&root.join("model").join("qwen"), 0o755);
    assert!(fs::create_dir_all(root.join("model").join("qwen.d")).is_ok());
    assert!(symlink("qwen", root.join("model").join("main")).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    let root_node = projection.root_node();
    let root_node = ok!(root_node);
    assert_eq!(root_node.inode(), FUSE_V1_ROOT_INODE);
    assert_eq!(root_node.abi_path(), "");
    assert_eq!(root_node.attr().file_type(), FuseV1FileType::Directory);

    let root_attr = projection.getattr_node(&root_node);
    assert!(matches!(
        root_attr,
        Ok(ref attr)
            if attr.abi_path().is_empty()
                && attr.file_type() == FuseV1FileType::Directory
    ));

    let entries = projection.readdir_node(&root_node);
    let entries = ok!(entries);
    let names = entries
        .iter()
        .map(super::FuseV1DirEntry::name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["agent", "bin", "home", "model", "shared", "status", "tool"]
    );

    let model_node = projection.lookup(&root_node, "model");
    assert!(matches!(
        model_node,
        Ok(ref node)
            if node.abi_path() == "model"
                && node.attr().file_type() == FuseV1FileType::Directory
    ));
    let Ok(model_node) = model_node else { return };
    let model_entries = projection.readdir("model");
    let model_entries = ok!(model_entries);
    let model_names = model_entries
        .iter()
        .map(super::FuseV1DirEntry::name)
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["debug", "helper", "main", "route"]);
    let main_node = projection.lookup(&model_node, "main");
    assert!(matches!(
        main_node,
        Ok(ref node)
            if node.abi_path() == "model/main"
                && node.attr().file_type() == FuseV1FileType::Symlink
    ));
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/openai/gpt-5.5"))
    );
    assert_eq!(
        projection.readlink("model/helper"),
        Ok(PathBuf::from("/ctx/model/openai/codex-auto-review"))
    );
    assert!(
        projection
            .set_model_alias("model/main", Path::new("api.lmm.best/gpt-5.4"))
            .is_ok()
    );
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/api.lmm.best/gpt-5.4"))
    );
    assert_eq!(
        projection.set_model_alias("model/test", Path::new("api.lmm.best/gpt-5.4")),
        Err(FuseV1Error::NotControlFile)
    );
    assert_eq!(
        projection.set_model_alias("model/main", Path::new("main")),
        Err(FuseV1Error::InvalidPath)
    );
    assert!(projection.remove_model_alias("model/main").is_ok());
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/openai/gpt-5.5"))
    );
    let debug_node = projection.lookup(&model_node, "debug");
    assert!(matches!(
        debug_node,
        Ok(ref node)
            if node.abi_path() == "model/debug"
                && node.inode() != FUSE_V1_ROOT_INODE
                && node.attr().file_type() == FuseV1FileType::Directory
    ));
    let Ok(debug_node) = debug_node else { return };
    let debug_entries = projection.readdir("model/debug");
    let debug_entries = ok!(debug_entries);
    let debug_names = debug_entries
        .iter()
        .map(super::FuseV1DirEntry::name)
        .collect::<Vec<_>>();
    assert_eq!(debug_names, ["echo", "echo.d"]);
    let echo_node = projection.lookup(&debug_node, "echo");
    assert!(matches!(
        echo_node,
        Ok(ref node)
            if node.abi_path() == "model/debug/echo"
                && node.inode() != FUSE_V1_ROOT_INODE
                && node.attr().file_type() == FuseV1FileType::Regular
    ));
    let echo_again = projection.node_for_path("model/debug/echo");
    assert!(
        matches!((echo_node, echo_again), (Ok(ref left), Ok(ref right)) if left.inode() == right.inode())
    );
    let echo_metadata = projection.read_to_string("model/debug/echo");
    assert!(matches!(
        echo_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.name=debug/echo\n")
    ));
    assert_eq!(
        projection.read_at("model/debug/echo", 0, 32),
        Ok(echo_metadata
            .unwrap_or_default()
            .bytes()
            .take(32)
            .collect::<Vec<_>>())
    );
    let echo_attr = projection.getattr("model/debug/echo");
    assert!(matches!(
        echo_attr,
        Ok(ref attr) if attr.mode() & 0o777 == 0o555
    ));
    let tool_metadata = projection.read_to_string("tool/tsh");
    assert!(matches!(
        tool_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.object=tool\n")
                && content.contains("# cortexfs.name=tsh\n")
                && !content.contains("#!/bin/sh")
    ));
    let agent_metadata = projection.read_to_string("agent/coder");
    assert!(matches!(
        agent_metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.object=agent\n")
                && content.contains("# cortexfs.name=coder\n")
                && content.contains("# cortexfs.model=main\n")
                && !content.contains("reference-tree agent stub")
    ));
    let tool_attr = projection.getattr("tool/tsh");
    assert!(matches!(
        tool_attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Regular && attr.mode() & 0o777 == 0o555
    ));
    let agent_attr = projection.getattr("agent/coder");
    assert!(matches!(
        agent_attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Regular && attr.mode() & 0o777 == 0o555
    ));
    assert_eq!(
        projection.lookup(&root_node, "../escape"),
        Err(FuseV1Error::InvalidPath)
    );
    assert_eq!(
        projection.lookup(&root_node, "missing"),
        Err(FuseV1Error::NotFound)
    );

    assert_eq!(
        projection.getattr("model/debug/echo.sock"),
        Err(FuseV1Error::NotFound)
    );
    let socket_attr = projection.getattr("agent/coder.sock");
    assert!(matches!(
        socket_attr,
        Ok(ref attr)
            if attr.file_type() == FuseV1FileType::Socket && attr.mode() & 0o777 == 0o777
    ));
    assert_eq!(
        projection.getattr("home/1000/tool/fs.read"),
        Err(FuseV1Error::NotFound)
    );
}

#[test]
fn fuse_v1_projection_model_alias_does_not_reuse_predictable_temp_symlink() {
    let root = reference_tree("fuse-v1-model-alias-temp");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let old_predictable_temp = root
        .join("model")
        .join(format!(".main.tmp.{}", std::process::id()));
    assert!(symlink("/ctx/model/keep", &old_predictable_temp).is_ok());

    assert!(
        projection
            .set_model_alias("model/main", Path::new("api.lmm.best/gpt-5.4"))
            .is_ok()
    );

    assert!(matches!(
        fs::read_link(&old_predictable_temp),
        Ok(ref target) if target == Path::new("/ctx/model/keep")
    ));
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/api.lmm.best/gpt-5.4"))
    );
    let temp_leftovers = fs::read_dir(root.join("model")).map_or(usize::MAX, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".main.tmp-")
            })
            .count()
    });
    assert_eq!(temp_leftovers, 0);
}

#[test]
fn fuse_v1_projection_renames_model_alias_symlink_atomically() {
    let root = reference_tree("fuse-v1-model-alias-rename");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert!(
        projection
            .set_model_alias_symlink("model/tmp", Path::new("api.lmm.best/gpt-5.4"))
            .is_ok()
    );
    assert!(
        projection
            .rename_model_alias_symlink("model/tmp", "model/main")
            .is_ok()
    );

    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/api.lmm.best/gpt-5.4"))
    );
    assert!(!root.join("model").join("tmp").exists());
}

#[test]
fn fuse_v1_projection_model_alias_rejects_symlink_model_directory_without_touching_target() {
    let root = clean_test_dir("fuse-v1-model-alias-symlink-model");
    let outside = clean_test_dir("fuse-v1-model-alias-symlink-model-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, root.join("model")).is_ok());
    assert!(symlink("/ctx/model/keep", outside.join("main")).is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/openai/gpt-5.5"))
    );
    assert_eq!(
        projection.set_model_alias("model/main", Path::new("api.lmm.best/gpt-5.4")),
        Err(FuseV1Error::Io)
    );
    assert_eq!(
        projection.remove_model_alias("model/main"),
        Err(FuseV1Error::Io)
    );
    assert!(matches!(
        fs::read_link(outside.join("main")),
        Ok(ref target) if target == Path::new("/ctx/model/keep")
    ));
    let temp_leftovers = fs::read_dir(&outside).map_or(usize::MAX, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".main.tmp-")
            })
            .count()
    });
    assert_eq!(temp_leftovers, 0);
}

#[test]
fn fuse_v1_projection_read_at_uses_linux_file_read_shape() {
    let root = reference_tree("fuse-v1-read-at");
    assert!(fs::write(root.join("shared").join("readable"), b"abcdef").is_ok());
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.read_at("shared/readable", 2, 3),
        Ok(b"cde".to_vec())
    );
    assert_eq!(projection.read_at("shared/readable", 32, 8), Ok(Vec::new()));
    assert_eq!(
        projection.read_at("shared", 0, 8),
        Err(FuseV1Error::NotFile)
    );
}

#[test]
fn fuse_v1_projection_write_control_file_at_enforces_whole_text_control_writes() {
    let root = reference_tree("fuse-v1-write-control-at");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    assert_eq!(
        projection.write_control_file_at("agent/coder.d/label", 1, b"worker"),
        Err(FuseV1Error::InvalidOffset)
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/label", 0, &[0xff]),
        Err(FuseV1Error::InvalidContent)
    );
    assert_eq!(
        projection.write_control_file_at("status", 0, b"worker"),
        Err(FuseV1Error::NotControlFile)
    );
    assert!(
        projection
            .write_control_file_at("agent/coder.d/label", 0, b"worker")
            .is_ok()
    );
    assert!(matches!(
        projection.read_to_string("agent/coder.d/label"),
        Ok(ref content) if content == "worker"
    ));
}

#[test]
fn fuse_v1_projection_allows_agent_log_append_at_file_end_only() {
    let root = reference_tree("fuse-v1-agent-log-append");
    let projection =
        FuseV1Projection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));
    let log = root.join("agent").join("coder.d").join("log");
    let offset = fs::metadata(&log).map_or(0, |metadata| metadata.len());

    assert!(
        projection
            .write_control_file_at("agent/coder.d/log", offset, b"{\"type\":\"agent.start\"}\n")
            .is_ok()
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/log", offset, b"stale\n"),
        Err(FuseV1Error::InvalidOffset)
    );
    assert_eq!(
        projection.write_control_file_at("agent/coder.d/status", 1, b"ready\n"),
        Err(FuseV1Error::InvalidOffset)
    );
    assert!(matches!(
        projection.read_to_string("agent/coder.d/log"),
        Ok(ref content) if content.contains("\"type\":\"agent.start\"")
    ));
}
use super::*;
