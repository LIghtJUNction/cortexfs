#[test]
fn shared_queue_finish_writes_readable_done_result_and_cleans_lease() {
    let root = clean_test_dir("shared-queue-finish-done");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    let result_path =
        finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n");
    assert_eq!(
        result_path,
        Ok(root.join("done").join("job-1.req.json.result"))
    );
    let result = fs::read_to_string(root.join("done").join("job-1.req.json.result"));
    assert!(matches!(result, Ok(ref content) if content == "ok\n"));
    let request = fs::read_to_string(root.join("done").join("job-1.req.json"));
    assert!(matches!(request, Ok(ref content) if content == "one\n"));
    assert!(!root.join("claimed").join("job-1.req.json").exists());
    assert!(!root.join("lease").join("job-1.req.json").exists());
}

#[test]
fn shared_queue_finish_writes_readable_failed_result() {
    let root = clean_test_dir("shared-queue-finish-failed");
    create_shared_queue_layout(&root);
    write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

    let claimed = claim_next_shared_queue_job(&root, "worker-a");
    let Some(claimed) = ok!(claimed) else { return };
    let result_path = finish_shared_queue_job(
        &root,
        claimed.job_name(),
        SharedQueueOutcome::Failed,
        b"err\n",
    );
    assert_eq!(
        result_path,
        Ok(root.join("failed").join("job-1.req.json.result"))
    );
    let result = fs::read_to_string(root.join("failed").join("job-1.req.json.result"));
    assert!(matches!(result, Ok(ref content) if content == "err\n"));
    let request = fs::read_to_string(root.join("failed").join("job-1.req.json"));
    assert!(matches!(request, Ok(ref content) if content == "one\n"));
}

#[test]
fn shared_access_authority_requires_mount_linux_permission_and_policy() {
    let root = clean_test_dir("shared-authority-ok");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o400);

    let metadata = fs::metadata(&file);
    let metadata = ok!(metadata);
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_source_target(
        "/ctx/shared/project-a",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);
    let authority = SharedAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Read, authority),
        Ok(())
    );
}

#[test]
fn shared_access_authority_denies_write_on_read_only_mount() {
    let root = clean_test_dir("shared-authority-ro");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o600);

    let metadata = fs::metadata(&file);
    let metadata = ok!(metadata);
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts =
        mount_table_for_source_target("/ctx/shared/project-a", &shared, "ro", "bind,nosuid,nodev");
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Write);
    let authority = SharedAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Write, authority),
        Err(SharedAccessDenial::ReadOnlyMount)
    );
}

#[test]
fn shared_access_authority_denies_missing_policy_and_wrong_space() {
    let root = clean_test_dir("shared-authority-policy");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o400);

    let metadata = fs::metadata(&file);
    let metadata = ok!(metadata);
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts =
        mount_table_for_source_target("/ctx/shared/project-a", &shared, "ro", "bind,nosuid,nodev");
    let wrong_mounts =
        mount_table_for_source_target("/ctx/shared/project-b", &shared, "ro", "bind,nosuid,nodev");
    let empty_policy = PolicyV0::parse("");
    let empty_policy = ok!(empty_policy);
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);

    assert_eq!(
        authorize_shared_access(
            "project-a",
            &file,
            SharedAccess::Read,
            SharedAccessAuthority::new(&identity, &mounts, "coder_t", &empty_policy),
        ),
        Err(SharedAccessDenial::Policy)
    );
    assert_eq!(
        authorize_shared_access(
            "project-a",
            &file,
            SharedAccess::Read,
            SharedAccessAuthority::new(&identity, &wrong_mounts, "coder_t", &policy),
        ),
        Err(SharedAccessDenial::WrongSharedPath)
    );
    assert_eq!(
        authorize_shared_access(
            "project-a",
            &file,
            SharedAccess::Read,
            SharedAccessAuthority::new(&identity, &MountTable::default(), "coder_t", &policy,),
        ),
        Err(SharedAccessDenial::NotMounted)
    );
}

#[test]
fn shared_access_authority_checks_linux_mode_bits() {
    let root = clean_test_dir("shared-authority-linux");
    let shared = root.join("shared-project-a");
    let file = shared.join("data.txt");
    assert!(fs::create_dir_all(&shared).is_ok());
    write_fixture_file(&file, 0o400);

    let metadata = fs::metadata(&file);
    let metadata = ok!(metadata);
    let other_identity = AgentUnixIdentity::new(
        metadata.uid().saturating_add(1),
        metadata.gid().saturating_add(1),
        [],
    );
    let mounts =
        mount_table_for_source_target("/ctx/shared/project-a", &shared, "ro", "bind,nosuid,nodev");
    let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);
    let authority = SharedAccessAuthority::new(&other_identity, &mounts, "coder_t", &policy);

    assert_eq!(
        authorize_shared_access("project-a", &file, SharedAccess::Read, authority),
        Err(SharedAccessDenial::LinuxPermission)
    );
}

#[test]
fn session_access_authority_allows_explicit_im_channel_session() {
    let root = clean_test_dir("session-authority-im-ok");
    let shared = root.join("im-qq-dev");
    let messages = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-456")
        .join("messages.jsonl");
    write_fixture_file(&messages, 0o600);

    let metadata = fs::metadata(&messages);
    let metadata = ok!(metadata);
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_source_target(
        "/ctx/shared/im-qq-dev",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = policy_with_rules([
        "allow bot_t shared:im-qq-dev read",
        "allow bot_t session:group-456 read",
    ]);
    let authority = SessionAccessAuthority::new(&identity, &mounts, "bot_t", &policy);

    assert_eq!(
        authorize_session_access(&messages, SessionAccess::Read, authority),
        Ok(())
    );
}

#[test]
fn session_access_authority_denies_cross_channel_without_session_policy() {
    let root = clean_test_dir("session-authority-im-deny");
    let shared = root.join("im-qq-dev");
    let allowed = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-456")
        .join("messages.jsonl");
    let other = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("group-999")
        .join("messages.jsonl");
    write_fixture_file(&allowed, 0o600);
    write_fixture_file(&other, 0o600);

    let metadata = fs::metadata(&allowed);
    let metadata = ok!(metadata);
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_source_target(
        "/ctx/shared/im-qq-dev",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = policy_with_rules([
        "allow bot_t shared:im-qq-dev read",
        "allow bot_t session:group-456 read",
    ]);
    let authority = SessionAccessAuthority::new(&identity, &mounts, "bot_t", &policy);

    assert_eq!(
        authorize_session_access(&allowed, SessionAccess::Read, authority),
        Ok(())
    );
    assert_eq!(
        authorize_session_access(&other, SessionAccess::Read, authority),
        Err(SessionAccessDenial::SessionPolicy)
    );
    assert_eq!(SessionAccessDenial::SessionPolicy.errno(), "EACCES");
}

#[test]
fn session_access_authority_requires_shared_policy_and_mount_write_mode() {
    let root = clean_test_dir("session-authority-shared-policy");
    let shared = root.join("im-slack-company");
    let messages = shared
        .join("agent")
        .join("bot")
        .join("session")
        .join("channel-789")
        .join("messages.jsonl");
    write_fixture_file(&messages, 0o600);

    let metadata = fs::metadata(&messages);
    let metadata = ok!(metadata);
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let ro_mounts = mount_table_for_source_target(
        "/ctx/shared/im-slack-company",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let writable_mounts = mount_table_for_source_target(
        "/ctx/shared/im-slack-company",
        &shared,
        "rw",
        "bind,nosuid,nodev",
    );
    let session_only = policy_with_rules(["allow bot_t session:channel-789 read"]);
    let read_policy = policy_with_rules([
        "allow bot_t shared:im-slack-company read",
        "allow bot_t session:channel-789 write",
    ]);

    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Read,
            SessionAccessAuthority::new(&identity, &ro_mounts, "bot_t", &session_only),
        ),
        Err(SessionAccessDenial::SharedPolicy)
    );
    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Write,
            SessionAccessAuthority::new(&identity, &ro_mounts, "bot_t", &read_policy),
        ),
        Err(SessionAccessDenial::ReadOnlyMount)
    );
    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Write,
            SessionAccessAuthority::new(&identity, &writable_mounts, "bot_t", &read_policy),
        ),
        Err(SessionAccessDenial::SharedPolicy)
    );
}

#[test]
fn session_access_authority_enforces_private_home_uid() {
    let root = clean_test_dir("session-authority-private-uid");
    let home = root.join("home-1000");
    let messages = home
        .join("agent")
        .join("coder")
        .join("session")
        .join("default")
        .join("messages.jsonl");
    write_fixture_file(&messages, 0o644);

    let metadata = fs::metadata(&messages);
    let metadata = ok!(metadata);
    let owner_identity = AgentUnixIdentity::new(1000, metadata.gid(), []);
    let other_identity = AgentUnixIdentity::new(1001, metadata.gid(), []);
    let mounts =
        mount_table_for_source_target("/ctx/home/1000", &home, "ro", "bind,nosuid,nodev,noexec");
    let policy = policy_with_rules(["allow coder_t session:default read"]);

    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Read,
            SessionAccessAuthority::new(&owner_identity, &mounts, "coder_t", &policy),
        ),
        Ok(())
    );
    assert_eq!(
        authorize_session_access(
            &messages,
            SessionAccess::Read,
            SessionAccessAuthority::new(&other_identity, &mounts, "coder_t", &policy),
        ),
        Err(SessionAccessDenial::LinuxPermission)
    );
}

#[test]
fn session_access_authority_rejects_unmounted_and_non_session_paths() {
    let root = clean_test_dir("session-authority-path-shape");
    let shared = root.join("project-a");
    let file = shared.join("data").join("note.txt");

    write_fixture_file(&file, 0o644);

    let metadata = fs::metadata(&file);
    let metadata = ok!(metadata);
    let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let mounts = mount_table_for_source_target(
        "/ctx/shared/project-a",
        &shared,
        "ro",
        "bind,nosuid,nodev,noexec",
    );
    let policy = policy_with_rules([
        "allow coder_t shared:project-a read",
        "allow coder_t session:default read",
    ]);

    assert_eq!(
        authorize_session_access(
            &file,
            SessionAccess::Read,
            SessionAccessAuthority::new(&identity, &mounts, "coder_t", &policy),
        ),
        Err(SessionAccessDenial::InvalidSessionPath)
    );
    assert_eq!(
        authorize_session_access(
            &file,
            SessionAccess::Read,
            SessionAccessAuthority::new(&identity, &MountTable::default(), "coder_t", &policy),
        ),
        Err(SessionAccessDenial::NotMounted)
    );
}

#[test]
fn ctx_path_parses_without_implicit_current_directory() {
    let path = ToolPath::parse(":/ctx/tool::/ctx/home/1000/tool:");
    assert_eq!(
        path.dirs(),
        [
            PathBuf::from("/ctx/tool"),
            PathBuf::from("/ctx/home/1000/tool")
        ]
    );
}

#[test]
fn tool_lookup_uses_first_executable_hit() {
    let root = clean_test_dir("tool-lookup");
    let global = root.join("global-tool");
    let user = root.join("user-tool");
    assert!(fs::create_dir_all(&global).is_ok());
    assert!(fs::create_dir_all(&user).is_ok());

    write_fixture_file(&global.join("fs.read"), 0o644);
    write_fixture_file(&global.join("fs.write"), 0o755);
    write_fixture_file(&user.join("fs.read"), 0o755);
    assert!(fs::create_dir_all(user.join("fs.read.d")).is_ok());

    let path = ToolPath::new([global.clone(), user.clone()]);
    let found = path.find("fs.read");
    assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == user.join("fs.read")));
    assert!(matches!(found, Ok(Some(ref hit)) if hit.control_dir() == user.join("fs.read.d")));

    write_fixture_file(&global.join("fs.read"), 0o755);
    let found = path.find("fs.read");
    assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == global.join("fs.read")));
}
