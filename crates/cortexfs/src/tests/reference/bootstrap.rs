pub(super) fn assert_reference_bin_placeholders(root: &Path) {
    assert_file_text(
        &root.join("bin").join("ctx"),
        "#!/bin/sh\n# CortexFS reference-tree ctx placeholder.\nexec /usr/bin/ctx \"$@\"\n",
    );
    assert_file_text(
        &root.join("bin").join("ctxterm"),
        "#!/bin/sh\n# CortexFS reference-tree ctxterm placeholder.\nexec /usr/bin/ctxterm \"$@\"\n",
    );
    assert_file_text(
        &root.join("bin").join("tsh"),
        "#!/bin/sh\n# CortexFS reference-tree tsh placeholder.\nexec /usr/bin/tsh \"$@\"\n",
    );
    assert_file_text(
        &root.join("bin").join("cortexfs-object-runner"),
        "#!/bin/sh\n# CortexFS reference-tree cortexfs-object-runner placeholder.\nexec /usr/bin/cortexfs-object-runner \"$@\"\n",
    );
}

#[test]
fn reference_tree_bootstrap_repairs_control_file_modes() {
    let root = clean_test_dir("reference-tree-control-mode");
    let status = root.join("tool").join("tsh.d").join("status");
    assert!(fs::create_dir_all(status.parent().unwrap_or(root.as_path())).is_ok());
    assert!(fs::write(&status, "idle\n").is_ok());
    assert!(fs::set_permissions(&status, fs::Permissions::from_mode(0o600)).is_ok());

    assert!(ensure_reference_tree(&root).is_ok());

    let mode = fs::metadata(status).map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(mode, Ok(0o644)));
}

#[test]
fn reference_tree_bootstrap_replaces_tshrc_symlink_without_chmodding_target() {
    let root = clean_test_dir("reference-tree-tshrc-symlink");
    let victim = clean_test_dir("reference-tree-tshrc-victim");
    let victim_target = victim.join("target");
    assert!(fs::create_dir_all(&victim).is_ok());
    assert!(fs::write(&victim_target, "keep-private\n").is_ok());
    assert!(fs::set_permissions(&victim_target, fs::Permissions::from_mode(0o600)).is_ok());

    let tshrc = ctx_home(&root).join(".tshrc");
    assert!(fs::create_dir_all(ctx_home(&root)).is_ok());
    assert!(symlink(&victim_target, &tshrc).is_ok());

    let bootstrapped = ensure_reference_tree(&root);
    assert!(bootstrapped.is_ok());

    let target_mode =
        fs::metadata(&victim_target).map(|metadata| metadata.permissions().mode() & 0o777);
    assert!(matches!(target_mode, Ok(0o600)));
    let tshrc_metadata = ok!(fs::symlink_metadata(&tshrc));
    assert!(tshrc_metadata.is_file());
    assert_eq!(tshrc_metadata.permissions().mode() & 0o777, 0o644);
    assert_file_text(&tshrc, "CTX_PATH=/ctx/tool:/ctx/home/1000/tool\n");
}

#[test]
fn reference_tree_bootstrap_rejects_symlinked_home_directory_without_writing_target() {
    let root = clean_test_dir("reference-tree-home-dir-symlink");
    let outside = clean_test_dir("reference-tree-home-dir-symlink-outside");
    assert!(fs::create_dir_all(root.join("home")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, root.join("home").join("1000")).is_ok());

    assert_eq!(
        ensure_reference_tree(&root),
        Err(ReferenceTreeError::CannotCreate)
    );
    assert!(!outside.join(".tshrc").exists());
    assert!(!outside.join("agent").exists());
    assert!(!outside.join("tool").exists());
}

#[test]
fn reference_tree_bootstrap_rejects_symlinked_home_parent_without_writing_target() {
    let root = clean_test_dir("reference-tree-home-parent-symlink");
    let outside = clean_test_dir("reference-tree-home-parent-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, root.join("home")).is_ok());

    assert_eq!(
        ensure_reference_tree(&root),
        Err(ReferenceTreeError::CannotCreate)
    );
    assert!(!outside.join("1000").exists());
    assert!(!outside.join("cortexfs-docs").exists());
}

#[test]
fn reference_tree_bootstrap_does_not_chown_descendants_through_symlink() {
    if !nix::unistd::Uid::effective().is_root() {
        return;
    }

    let root = clean_test_dir("reference-tree-chown-symlink-race");
    let victim = clean_test_dir("reference-tree-chown-victim");
    assert!(fs::create_dir_all(&victim).is_ok());
    let victim_target = victim.join("target");
    assert!(fs::write(&victim_target, "keep-root-owned\n").is_ok());
    assert!(
        nix::unistd::chown(
            &victim_target,
            Some(nix::unistd::Uid::from_raw(0)),
            Some(nix::unistd::Gid::from_raw(0)),
        )
        .is_ok()
    );

    let attacker_link = ctx_home(&root).join("attacker-link");
    assert!(fs::create_dir_all(ctx_home(&root)).is_ok());
    assert!(symlink(&victim, &attacker_link).is_ok());

    let bootstrapped = ensure_reference_tree(&root);
    assert!(bootstrapped.is_ok());

    let metadata = ok!(fs::symlink_metadata(&victim_target));
    assert_eq!(metadata.uid(), 0);
    assert_eq!(metadata.gid(), 0);
}

#[test]
fn reference_tree_model_exec_is_readonly_metadata() {
    let root = reference_tree("reference-tree-model-metadata");
    let projection =
        FuseProjection::new(&root).with_provider_config_dir(root.join("missing-providers.d"));

    let metadata = projection.read_to_string("model/debug/echo");
    let metadata = ok!(metadata);
    assert!(metadata.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n")));
    assert!(metadata.contains("# cortexfs.object=model\n"));
    assert!(metadata.contains("# cortexfs.id=debug/echo\n"));
    assert!(metadata.contains("# cortexfs.name=debug/echo\n"));
    assert!(metadata.contains("# cortexfs.description=Built-in debug echo model\n"));
    assert!(metadata.contains("# cortexfs.type=debug\n"));
    assert!(metadata.contains("# cortexfs.created_at=\n"));
    assert!(metadata.contains("# cortexfs.owned_by=cortexfs\n"));
    assert!(metadata.contains("# cortexfs.context_length=unknown\n"));
    assert!(metadata.contains("# cortexfs.driver=debug\n"));
    assert!(metadata.contains("# cortexfs.driver.default=debug\n"));
    assert!(metadata.contains("# cortexfs.driver.exec=debug\n"));
    assert!(metadata.contains("# cortexfs.driver.socket=\n"));
    assert!(metadata.contains("# cortexfs.driver.agent=debug\n"));
    let permissions = projection
        .getattr("model/debug/echo")
        .map(|attr| attr.mode() & 0o777);
    assert!(matches!(permissions, Ok(0o555)));
    let driver_permissions = projection
        .getattr("model/debug/echo.d/driver")
        .map(|attr| attr.mode() & 0o777);
    assert!(matches!(driver_permissions, Ok(0o644)));
}

#[test]
fn reference_tree_bootstrap_materializes_current_layout() {
    let root = clean_test_dir("reference-tree-current-layout");

    assert!(ensure_reference_tree(&root).is_ok());

    assert_file_text(&root.join("agent").join("coder.d").join("model"), "main\n");
    let coder_system = ok!(fs::read_to_string(
        root.join("agent").join("coder.d").join("system.md")
    ));
    assert!(coder_system.contains("default Architect -> coder/reviewer flow"));
    assert!(coder_system.contains("fs.write"));
    assert!(coder_system.contains("shell.exec"));
    let architect_system = ok!(fs::read_to_string(
        root.join("agent").join("architect.d").join("system.md")
    ));
    assert!(architect_system.contains("human role name is Architect"));
    assert!(architect_system.contains("delegate implementation to `coder`"));
    let reviewer_system = ok!(fs::read_to_string(
        root.join("agent").join("reviewer.d").join("system.md")
    ));
    assert!(reviewer_system.contains("independent review agent"));
    assert!(root.join("agent").join("worker.d").is_dir());
    let prompt_template = fs::read_to_string(
        root.join("agent")
            .join("coder.d")
            .join("prompt.template.md"),
    );
    assert!(
        matches!(prompt_template, Ok(ref content) if content.contains("{{agent_instructions}}"))
    );
    let agent_script = fs::read_to_string(root.join("agent").join("coder"));
    assert!(
        matches!(agent_script, Ok(ref content) if content.contains("# cortexfs.object=agent\n")
            && content.contains("# cortexfs.name=coder\n")
            && content.contains("cortexfs-object-runner"))
    );
    let agent_policy = fs::read_to_string(root.join("agent").join("coder.d").join("policy"));
    assert!(matches!(agent_policy, Ok(ref content) if content.contains("model:main use")));
    assert!(
        matches!(agent_policy, Ok(ref content) if !content.contains("network:default connect"))
    );
}
use super::*;
